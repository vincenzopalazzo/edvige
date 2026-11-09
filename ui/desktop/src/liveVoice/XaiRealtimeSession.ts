import { acpChatSessionActions, acpChatSessionStore } from '../acp/chatSessionStore';
import { acpChatSessionController } from '../acp/chatSessionController';
import { createUserMessage } from '../types/message';

const WORKLET_URL = new URL('audio-capture-worklet.js', window.location.href.split('#')[0]).href;
const SAMPLE_RATE = 24_000;
const DELEGATE_TOOL = 'delegate_to_goose';
const SPOKEN_RESULT_LIMIT = 500;

const INSTRUCTIONS = [
  'You are goose speaking out loud.',
  'For small talk, answer yourself in one or two sentences.',
  'For anything that needs files, code, tools, or the current session, call delegate_to_goose with the user request and wait.',
  'After that tool returns, tell the user the outcome in a short spoken sentence. Do not invent tool results.',
].join(' ');

type SocketEvent = {
  type?: string;
  name?: string;
  call_id?: string;
  arguments?: string | { request?: string };
  delta?: string;
  audio?: string;
};

export class XaiRealtimeSession {
  private socket: WebSocket | null = null;
  private audioContext: AudioContext | null = null;
  private stream: MediaStream | null = null;
  private worklet: AudioWorkletNode | null = null;
  private playTime = 0;
  private muted = false;
  private closed = false;
  private sampleRate = SAMPLE_RATE;
  private pendingChunks: Float32Array[] = [];
  private pendingSamples = 0;
  private sessionReady = false;

  constructor(
    private readonly sessionId: string,
    private readonly onFailure: () => void,
    private readonly primedAudioContext: AudioContext | null = null
  ) {}

  async connect(websocketUrl: string, clientSecret: string): Promise<void> {
    const socket = new WebSocket(websocketUrl, [`xai-client-secret.${clientSecret}`]);
    this.socket = socket;
    socket.binaryType = 'arraybuffer';
    await new Promise<void>((resolve, reject) => {
      socket.onopen = () => resolve();
      socket.onerror = () => reject(new Error('Grok live voice failed to connect'));
    });
    if (this.closed) {
      socket.close();
      return;
    }
    socket.onmessage = (event) => {
      if (typeof event.data !== 'string') return;
      this.onMessage(event.data);
    };
    socket.onclose = () => {
      if (!this.closed) this.onFailure();
    };
    await this.startMicrophone();
    if (this.closed) return;
    this.send({
      type: 'session.update',
      session: {
        voice: 'eve',
        instructions: INSTRUCTIONS,
        turn_detection: { type: 'server_vad' },
        audio: {
          input: { format: { type: 'audio/pcm', rate: this.sampleRate } },
          output: { format: { type: 'audio/pcm', rate: this.sampleRate } },
        },
        tools: [
          {
            type: 'function',
            name: DELEGATE_TOOL,
            description: 'Hand the user request to goose so it can use tools and the session.',
            parameters: {
              type: 'object',
              properties: {
                request: { type: 'string', description: 'What the user asked for, in their words.' },
              },
              required: ['request'],
            },
          },
        ],
      },
    });
    this.sessionReady = true;
    this.flushAudio();
  }

  setMuted(muted: boolean) {
    this.muted = muted;
    for (const track of this.stream?.getAudioTracks() ?? []) {
      track.enabled = !muted;
    }
  }

  sendCommentary(text: string) {
    this.send({
      type: 'conversation.item.create',
      item: {
        type: 'message',
        role: 'user',
        content: [{ type: 'input_text', text }],
      },
    });
    this.send({ type: 'response.create' });
  }

  teardown() {
    this.closed = true;
    this.sessionReady = false;
    this.pendingChunks = [];
    this.pendingSamples = 0;
    this.worklet?.disconnect();
    this.worklet = null;
    this.stream?.getTracks().forEach((track) => track.stop());
    this.stream = null;
    void this.audioContext?.close();
    this.audioContext = null;
    this.socket?.close();
    this.socket = null;
  }

  private async startMicrophone() {
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: { channelCount: 1, echoCancellation: true, noiseSuppression: true },
    });
    if (this.closed) {
      stream.getTracks().forEach((track) => track.stop());
      return;
    }
    this.stream = stream;
    const context = this.primedAudioContext ?? new AudioContext({ sampleRate: SAMPLE_RATE });
    this.audioContext = context;
    if (context.state === 'suspended') {
      await context.resume();
    }
    this.sampleRate = context.sampleRate || SAMPLE_RATE;
    this.playTime = context.currentTime;
    await context.audioWorklet.addModule(WORKLET_URL);
    const worklet = new AudioWorkletNode(context, 'audio-capture');
    this.worklet = worklet;
    worklet.port.onmessage = (event: MessageEvent<Float32Array>) => {
      if (this.muted || this.closed) return;
      this.pendingChunks.push(event.data);
      this.pendingSamples += event.data.length;
      if (this.sessionReady && this.pendingSamples >= this.sampleRate / 10) {
        this.flushAudio();
      }
    };
    const source = context.createMediaStreamSource(stream);
    const silence = context.createGain();
    silence.gain.value = 0;
    source.connect(worklet);
    worklet.connect(silence);
    silence.connect(context.destination);
  }

  private flushAudio() {
    if (this.pendingSamples === 0) return;
    const samples = new Float32Array(this.pendingSamples);
    let offset = 0;
    for (const chunk of this.pendingChunks) {
      samples.set(chunk, offset);
      offset += chunk.length;
    }
    this.pendingChunks = [];
    this.pendingSamples = 0;
    this.send({
      type: 'input_audio_buffer.append',
      audio: pcm16Base64(samples),
    });
  }

  private onMessage(raw: string) {
    let event: SocketEvent;
    try {
      event = JSON.parse(raw) as SocketEvent;
    } catch {
      return;
    }
    const audio = event.delta ?? event.audio;
    if (
      (event.type === 'response.output_audio.delta' || event.type === 'response.audio.delta') &&
      audio
    ) {
      this.playPcm16(audio);
      return;
    }
    if (event.type === 'response.function_call_arguments.done' && event.name === DELEGATE_TOOL) {
      void this.delegate(event);
    }
  }

  private async delegate(event: SocketEvent) {
    const callId = event.call_id;
    if (!callId || this.closed) return;
    const request = functionRequest(event.arguments);
    let output = 'Goose could not start that request.';
    try {
      output = await this.askGoose(request);
    } catch (error) {
      output = error instanceof Error ? error.message : output;
    }
    if (this.closed) return;
    this.send({
      type: 'conversation.item.create',
      item: {
        type: 'function_call_output',
        call_id: callId,
        output: JSON.stringify({ result: output }),
      },
    });
    await this.waitForPlayback();
    if (this.closed) return;
    this.send({ type: 'response.create' });
  }

  private async askGoose(request: string): Promise<string> {
    const snapshot = acpChatSessionStore.getSnapshot(this.sessionId);
    if (snapshot?.activePromptAttemptId) {
      return 'Goose is already working in this chat. Wait for that turn to finish.';
    }
    const message = createUserMessage(
      `[Spoken request] ${request}\n\nDo the work, then end with a short spoken summary. Do not include raw tool output.`
    );
    if (snapshot) {
      acpChatSessionActions.setMessages(this.sessionId, [...snapshot.messages, message]);
    }
    let failed: string | undefined;
    await acpChatSessionController.submitMessage(this.sessionId, message, {
      getCurrentSnapshot: () => acpChatSessionStore.getSnapshot(this.sessionId),
      onFinish: (outcome) => {
        if (outcome.status === 'failed') {
          failed = outcome.message || 'Goose could not finish that request.';
        }
      },
    });
    return failed ?? spokenSummary(this.sessionId);
  }

  private playPcm16(base64: string) {
    const context = this.audioContext;
    if (!context || this.closed) return;
    const bytes = Uint8Array.from(atob(base64), (char) => char.charCodeAt(0));
    if (bytes.byteLength < 2) return;
    const samples = new Float32Array(Math.floor(bytes.byteLength / 2));
    const view = new DataView(bytes.buffer, bytes.byteOffset, samples.length * 2);
    for (let i = 0; i < samples.length; i++) {
      samples[i] = view.getInt16(i * 2, true) / 0x8000;
    }
    const buffer = context.createBuffer(1, samples.length, this.sampleRate);
    buffer.copyToChannel(samples, 0);
    const source = context.createBufferSource();
    source.buffer = buffer;
    source.connect(context.destination);
    const startAt = Math.max(context.currentTime, this.playTime);
    source.start(startAt);
    this.playTime = startAt + buffer.duration;
  }

  private async waitForPlayback() {
    const context = this.audioContext;
    if (!context) return;
    const delayMs = Math.max(0, (this.playTime - context.currentTime) * 1000);
    if (delayMs === 0) return;
    await new Promise((resolve) => setTimeout(resolve, delayMs));
  }

  private send(payload: unknown) {
    if (this.socket?.readyState === WebSocket.OPEN) {
      this.socket.send(JSON.stringify(payload));
    }
  }
}

function functionRequest(args: SocketEvent['arguments']): string {
  if (!args) return '';
  if (typeof args === 'object') return args.request?.trim() || JSON.stringify(args);
  try {
    const parsed = JSON.parse(args) as { request?: string };
    return parsed.request?.trim() || args;
  } catch {
    return args;
  }
}

function spokenSummary(sessionId: string): string {
  const messages = acpChatSessionStore.getSnapshot(sessionId)?.messages ?? [];
  for (let i = messages.length - 1; i >= 0; i--) {
    const message = messages[i];
    if (message.role !== 'assistant' || !message.metadata.userVisible) continue;
    const text = message.content
      .flatMap((part) => (part.type === 'text' ? [part.text] : []))
      .join(' ')
      .replace(/\s+/g, ' ')
      .trim();
    if (text) return text.slice(0, SPOKEN_RESULT_LIMIT);
  }
  return 'Done. The result is in the chat.';
}

function pcm16Base64(samples: Float32Array): string {
  const bytes = new Uint8Array(samples.length * 2);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < samples.length; i++) {
    const clamped = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(i * 2, clamped < 0 ? clamped * 0x8000 : clamped * 0x7fff, true);
  }
  let binary = '';
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}
