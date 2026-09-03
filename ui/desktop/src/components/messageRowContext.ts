import {
  getAnyToolConfirmationData,
  getPendingToolConfirmationIds,
  getToolConfirmationContent,
  getToolRequests,
  getToolResponses,
  type Message,
  type ToolConfirmationData,
  type ToolResponseMessageContent,
} from '../types/message';
import { identifyConsecutiveToolCalls } from '../utils/toolCallChaining';

export interface ToolRenderState {
  requestId: string;
  response: ToolResponseMessageContent | undefined;
  confirmation: ToolConfirmationData | undefined;
  isPending: boolean;
}

export interface MessageRowContext {
  hideTimestamp: boolean;
  isInToolCallChain: boolean;
  previousResolvedModel: string | null;
  toolStates: readonly ToolRenderState[];
  toolConfirmationShownInline: boolean;
}

function resolvedModel(message: Message): string | null {
  if (message.role !== 'assistant' || !message.metadata.userVisible) return null;
  return message.metadata.inference?.resolvedModel ?? null;
}

function messageAffectsToolLookups(message: Message): boolean {
  return message.content.some(
    (content) =>
      content.type === 'toolRequest' ||
      content.type === 'toolResponse' ||
      content.type === 'toolConfirmationRequest' ||
      (content.type === 'actionRequired' && content.data.actionType === 'toolConfirmation')
  );
}

function sameMessageIdentities(left: Message[], right: Message[]): boolean {
  if (left === right) {
    return true;
  }
  if (left.length !== right.length) {
    return false;
  }
  return left.every((message, index) => message === right[index]);
}

function canReuseRowContexts(previous: Message[], next: Message[]): boolean {
  if (sameMessageIdentities(previous, next)) {
    return true;
  }
  if (previous.length !== next.length) {
    return false;
  }
  for (let i = 0; i < next.length - 1; i++) {
    if (previous[i] !== next[i]) {
      return false;
    }
  }
  const previousLast = previous[previous.length - 1];
  const nextLast = next[next.length - 1];
  return !messageAffectsToolLookups(previousLast) && !messageAffectsToolLookups(nextLast);
}

export function deriveMessageRowContexts(
  messages: Message[],
  previous?: { messages: Message[]; contexts: MessageRowContext[] }
): MessageRowContext[] {
  if (previous && canReuseRowContexts(previous.messages, messages)) {
    return previous.contexts;
  }
  const toolCallChains = identifyConsecutiveToolCalls(messages);
  const chainedMessageIndices = new Set<number>();
  const hiddenTimestampIndices = new Set<number>();

  for (const chain of toolCallChains) {
    for (const messageIndex of chain) {
      chainedMessageIndices.add(messageIndex);
    }
    for (const messageIndex of chain.slice(0, -1)) {
      hiddenTimestampIndices.add(messageIndex);
    }
  }

  const toolRequestIds = new Set<string>();
  const firstConfirmationByRequestId = new Map<string, ToolConfirmationData>();

  for (const message of messages) {
    for (const request of getToolRequests(message)) {
      toolRequestIds.add(request.id);
    }

    const confirmation = getAnyToolConfirmationData(message);
    if (confirmation && !firstConfirmationByRequestId.has(confirmation.id)) {
      firstConfirmationByRequestId.set(confirmation.id, confirmation);
    }
  }

  const pendingConfirmationIds = getPendingToolConfirmationIds(messages);
  const toolStatesByMessageIndex: ToolRenderState[][] = Array.from(
    { length: messages.length },
    () => []
  );
  const latestResponseByRequestId = new Map<string, ToolResponseMessageContent>();
  const latestResponseMessageIndexByRequestId = new Map<string, number>();

  for (let messageIndex = messages.length - 1; messageIndex >= 0; messageIndex--) {
    const message = messages[messageIndex];
    toolStatesByMessageIndex[messageIndex] = getToolRequests(message).map((request) => ({
      requestId: request.id,
      response: latestResponseByRequestId.get(request.id),
      confirmation: firstConfirmationByRequestId.get(request.id),
      isPending: pendingConfirmationIds.has(request.id),
    }));

    for (const response of getToolResponses(message)) {
      const existingResponseMessageIndex = latestResponseMessageIndexByRequestId.get(response.id);
      if (
        existingResponseMessageIndex === undefined ||
        existingResponseMessageIndex === messageIndex
      ) {
        latestResponseByRequestId.set(response.id, response);
        latestResponseMessageIndexByRequestId.set(response.id, messageIndex);
      }
    }
  }

  let previousResolvedModel: string | null = null;

  return messages.map((message, messageIndex) => {
    const currentResolvedModel = resolvedModel(message);
    const rowPreviousResolvedModel = currentResolvedModel ? previousResolvedModel : null;
    if (currentResolvedModel) previousResolvedModel = currentResolvedModel;

    const toolConfirmation = getToolConfirmationContent(message);
    const confirmationData = getAnyToolConfirmationData(message);

    return {
      hideTimestamp: hiddenTimestampIndices.has(messageIndex),
      isInToolCallChain: chainedMessageIndices.has(messageIndex),
      previousResolvedModel: rowPreviousResolvedModel,
      toolStates: toolStatesByMessageIndex[messageIndex],
      toolConfirmationShownInline: Boolean(
        toolConfirmation && confirmationData && toolRequestIds.has(confirmationData.id)
      ),
    };
  });
}
