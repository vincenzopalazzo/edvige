import { useCallback, useEffect, useState } from 'react';
import type { GatewayStatusDto } from '@aaif/goose-sdk';
import { Check, Copy, Radio, RefreshCw, ShieldCheck, Trash2, Unplug } from 'lucide-react';
import { defineMessages, useIntl } from '../../../i18n';
import {
  getGatewayStatus,
  pairSonarGateway,
  startSonarGateway,
  stopSonarGateway,
  unpairSonarGateway,
} from '../../../acp/gateways';
import { acpListRecentSessions, type SessionListItem } from '../../../acp/sessions';
import { useChatContext } from '../../../contexts/ChatContext';
import { Button } from '../../ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '../../ui/card';
import { Input } from '../../ui/input';

const DEFAULT_RELAY = 'wss://nostr.relay.hedwig.sh/';
const CONTROLLERS_STORAGE_KEY = 'goose.sonar.controllers';
const RELAYS_STORAGE_KEY = 'goose.sonar.relays';

const i18n = defineMessages({
  title: { id: 'sonarRemote.title', defaultMessage: 'Sonar remote control' },
  description: {
    id: 'sonarRemote.description',
    defaultMessage: 'Control Goose from an end-to-end encrypted Sonar group over Nostr.',
  },
  securityTitle: { id: 'sonarRemote.securityTitle', defaultMessage: 'Group access and authority' },
  securityDescription: {
    id: 'sonarRemote.securityDescription',
    defaultMessage:
      'Every group member can read future group traffic. Only controller npubs allowlisted below can execute Goose commands.',
  },
  keepOpen: {
    id: 'sonarRemote.keepOpen',
    defaultMessage: 'Keep this Goose Desktop window open while using Sonar remote control.',
  },
  controllers: { id: 'sonarRemote.controllers', defaultMessage: 'Controller npubs' },
  controllersHelp: {
    id: 'sonarRemote.controllersHelp',
    defaultMessage: 'Enter one or more Sonar identity npubs, separated by commas.',
  },
  controllersPlaceholder: {
    id: 'sonarRemote.controllersPlaceholder',
    defaultMessage: 'npub1…',
  },
  relays: { id: 'sonarRemote.relays', defaultMessage: 'Nostr relays' },
  relaysHelp: {
    id: 'sonarRemote.relaysHelp',
    defaultMessage: 'Enter one or more WebSocket relay URLs, separated by commas.',
  },
  start: { id: 'sonarRemote.start', defaultMessage: 'Start remote control' },
  update: { id: 'sonarRemote.update', defaultMessage: 'Update and restart' },
  starting: { id: 'sonarRemote.starting', defaultMessage: 'Starting…' },
  running: { id: 'sonarRemote.running', defaultMessage: 'Running' },
  stopped: { id: 'sonarRemote.stopped', defaultMessage: 'Stopped' },
  bridgeIdentity: { id: 'sonarRemote.bridgeIdentity', defaultMessage: 'Goose bridge identity' },
  bridgeHelp: {
    id: 'sonarRemote.bridgeHelp',
    defaultMessage: 'Invite this npub to a Sonar group from an allowlisted controller identity.',
  },
  copy: { id: 'sonarRemote.copy', defaultMessage: 'Copy' },
  copied: { id: 'sonarRemote.copied', defaultMessage: 'Copied' },
  refresh: { id: 'sonarRemote.refresh', defaultMessage: 'Refresh' },
  pairingTitle: { id: 'sonarRemote.pairingTitle', defaultMessage: 'Pair a session' },
  pairingHelp: {
    id: 'sonarRemote.pairingHelp',
    defaultMessage:
      'Choose an existing session, or create a dedicated remote session when the code is sent in Sonar.',
  },
  dedicatedSession: {
    id: 'sonarRemote.dedicatedSession',
    defaultMessage: 'Create a dedicated remote session',
  },
  currentSession: { id: 'sonarRemote.currentSession', defaultMessage: 'Current session' },
  generateCode: { id: 'sonarRemote.generateCode', defaultMessage: 'Generate pairing code' },
  generating: { id: 'sonarRemote.generating', defaultMessage: 'Generating…' },
  pairingCode: { id: 'sonarRemote.pairingCode', defaultMessage: 'Pairing code' },
  pairingExpiry: {
    id: 'sonarRemote.pairingExpiry',
    defaultMessage: 'Send this code in the Sonar group before {time}.',
  },
  pairedGroups: { id: 'sonarRemote.pairedGroups', defaultMessage: 'Paired groups' },
  noPairedGroups: { id: 'sonarRemote.noPairedGroups', defaultMessage: 'No groups paired yet.' },
  pairedSession: {
    id: 'sonarRemote.pairedSession',
    defaultMessage: 'Session {sessionId}',
  },
  revoke: { id: 'sonarRemote.revoke', defaultMessage: 'Revoke' },
  stop: { id: 'sonarRemote.stop', defaultMessage: 'Stop' },
  stopping: { id: 'sonarRemote.stopping', defaultMessage: 'Stopping…' },
  forget: { id: 'sonarRemote.forget', defaultMessage: 'Forget and revoke all' },
  forgetHelp: {
    id: 'sonarRemote.forgetHelp',
    defaultMessage:
      'Forget removes the saved gateway configuration and all Goose session pairings.',
  },
  controllersRequired: {
    id: 'sonarRemote.controllersRequired',
    defaultMessage: 'Enter at least one controller npub.',
  },
  error: { id: 'sonarRemote.error', defaultMessage: 'Sonar remote control failed: {error}' },
});

function splitValues(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export default function SonarRemoteControlSection() {
  const intl = useIntl();
  const chatContext = useChatContext();
  const currentSessionId = chatContext?.chat.sessionId ?? '';
  const [controllers, setControllers] = useState(
    () => window.localStorage.getItem(CONTROLLERS_STORAGE_KEY) ?? ''
  );
  const [relays, setRelays] = useState(
    () => window.localStorage.getItem(RELAYS_STORAGE_KEY) ?? DEFAULT_RELAY
  );
  const [gateway, setGateway] = useState<GatewayStatusDto>();
  const [sessions, setSessions] = useState<SessionListItem[]>([]);
  const [sessionId, setSessionId] = useState(currentSessionId);
  const [pairing, setPairing] = useState<{ code: string; expiresAt: number }>();
  const [busy, setBusy] = useState<'start' | 'pair' | 'stop' | 'forget' | 'unpair'>();
  const [error, setError] = useState<string>();
  const [copied, setCopied] = useState<'npub' | 'code'>();

  const refreshStatus = useCallback(async () => {
    const statuses = await getGatewayStatus();
    setGateway(statuses.find((status) => status.gatewayType === 'sonar'));
  }, []);

  useEffect(() => {
    setSessionId(currentSessionId);
  }, [currentSessionId]);

  useEffect(() => {
    let active = true;
    const load = async () => {
      try {
        const [statuses, recentSessions] = await Promise.all([
          getGatewayStatus(),
          acpListRecentSessions(100),
        ]);
        if (active) {
          setGateway(statuses.find((status) => status.gatewayType === 'sonar'));
          setSessions(recentSessions);
        }
      } catch (loadError) {
        if (active) {
          setError(errorMessage(loadError));
        }
      }
    };
    void load();
    const poll = window.setInterval(() => {
      void refreshStatus().catch(() => undefined);
    }, 5000);
    return () => {
      active = false;
      window.clearInterval(poll);
    };
  }, [refreshStatus]);

  const handleStart = async () => {
    const controllerValues = splitValues(controllers);
    if (controllerValues.length === 0) {
      setError(intl.formatMessage(i18n.controllersRequired));
      return;
    }
    setBusy('start');
    setError(undefined);
    setPairing(undefined);
    try {
      const status = await startSonarGateway(controllerValues, splitValues(relays));
      window.localStorage.setItem(CONTROLLERS_STORAGE_KEY, controllers);
      window.localStorage.setItem(RELAYS_STORAGE_KEY, relays);
      setGateway(status);
    } catch (startError) {
      setError(errorMessage(startError));
      await refreshStatus().catch(() => undefined);
    } finally {
      setBusy(undefined);
    }
  };

  const handlePair = async () => {
    setBusy('pair');
    setError(undefined);
    try {
      setPairing(await pairSonarGateway(sessionId || undefined));
    } catch (pairError) {
      setError(errorMessage(pairError));
    } finally {
      setBusy(undefined);
    }
  };

  const handleStop = async (forget: boolean) => {
    setBusy(forget ? 'forget' : 'stop');
    setError(undefined);
    try {
      await stopSonarGateway(forget);
      setPairing(undefined);
      await refreshStatus();
    } catch (stopError) {
      setError(errorMessage(stopError));
    } finally {
      setBusy(undefined);
    }
  };

  const handleUnpair = async (groupId: string) => {
    setBusy('unpair');
    setError(undefined);
    try {
      await unpairSonarGateway(groupId);
      await refreshStatus();
    } catch (unpairError) {
      setError(errorMessage(unpairError));
    } finally {
      setBusy(undefined);
    }
  };

  const handleCopy = async (kind: 'npub' | 'code', value: string) => {
    await navigator.clipboard.writeText(value);
    setCopied(kind);
    window.setTimeout(() => setCopied(undefined), 1500);
  };

  const running = gateway?.running ?? false;
  const bridgeNpub = gateway?.info?.npub;

  return (
    <section id="sonar-remote-control" className="space-y-4 pr-4 mt-1">
      <Card className="pb-2">
        <CardHeader className="pb-0">
          <div className="flex items-start justify-between gap-4">
            <div>
              <CardTitle className="flex items-center gap-2">
                <Radio className="size-5" />
                {intl.formatMessage(i18n.title)}
              </CardTitle>
              <CardDescription>{intl.formatMessage(i18n.description)}</CardDescription>
            </div>
            <span
              className={`rounded-full px-2.5 py-1 text-xs ${
                running
                  ? 'bg-green-100 text-green-800 dark:bg-green-950 dark:text-green-200'
                  : 'bg-background-secondary text-text-secondary'
              }`}
            >
              {intl.formatMessage(running ? i18n.running : i18n.stopped)}
            </span>
          </div>
        </CardHeader>
        <CardContent className="pt-4 space-y-5 px-4">
          <div className="rounded-md border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900 dark:border-amber-800 dark:bg-amber-950 dark:text-amber-100">
            <div className="flex gap-2">
              <ShieldCheck className="size-4 shrink-0" />
              <div>
                <p className="font-medium">{intl.formatMessage(i18n.securityTitle)}</p>
                <p className="mt-1">{intl.formatMessage(i18n.securityDescription)}</p>
                <p className="mt-1">{intl.formatMessage(i18n.keepOpen)}</p>
              </div>
            </div>
          </div>

          <div className="space-y-2">
            <label htmlFor="sonar-controllers" className="text-text-primary text-xs">
              {intl.formatMessage(i18n.controllers)}
            </label>
            <Input
              id="sonar-controllers"
              value={controllers}
              onChange={(event) => setControllers(event.target.value)}
              placeholder={intl.formatMessage(i18n.controllersPlaceholder)}
              disabled={Boolean(busy)}
              className="font-mono text-xs"
            />
            <p className="text-xs text-text-secondary">
              {intl.formatMessage(i18n.controllersHelp)}
            </p>
          </div>

          <div className="space-y-2">
            <label htmlFor="sonar-relays" className="text-text-primary text-xs">
              {intl.formatMessage(i18n.relays)}
            </label>
            <Input
              id="sonar-relays"
              value={relays}
              onChange={(event) => setRelays(event.target.value)}
              disabled={Boolean(busy)}
              className="font-mono text-xs"
            />
            <p className="text-xs text-text-secondary">{intl.formatMessage(i18n.relaysHelp)}</p>
          </div>

          <div className="flex flex-wrap gap-2">
            <Button onClick={() => void handleStart()} disabled={Boolean(busy)}>
              {busy === 'start'
                ? intl.formatMessage(i18n.starting)
                : intl.formatMessage(running ? i18n.update : i18n.start)}
            </Button>
            <Button variant="outline" onClick={() => void refreshStatus()} disabled={Boolean(busy)}>
              <RefreshCw /> {intl.formatMessage(i18n.refresh)}
            </Button>
          </div>

          {bridgeNpub && (
            <div className="rounded-md border p-3 space-y-2">
              <p className="text-xs font-medium">{intl.formatMessage(i18n.bridgeIdentity)}</p>
              <div className="flex items-center gap-2">
                <code className="min-w-0 flex-1 break-all text-xs">{bridgeNpub}</code>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => void handleCopy('npub', bridgeNpub)}
                >
                  {copied === 'npub' ? <Check /> : <Copy />}
                  {intl.formatMessage(copied === 'npub' ? i18n.copied : i18n.copy)}
                </Button>
              </div>
              <p className="text-xs text-text-secondary">{intl.formatMessage(i18n.bridgeHelp)}</p>
            </div>
          )}

          {running && (
            <div className="rounded-md border p-3 space-y-3">
              <div>
                <p className="text-xs font-medium">{intl.formatMessage(i18n.pairingTitle)}</p>
                <p className="text-xs text-text-secondary mt-1">
                  {intl.formatMessage(i18n.pairingHelp)}
                </p>
              </div>
              <select
                value={sessionId}
                onChange={(event) => setSessionId(event.target.value)}
                disabled={Boolean(busy)}
                className="h-9 w-full rounded-md border bg-background-primary px-3 text-sm text-text-primary focus:outline-none"
              >
                <option value="">{intl.formatMessage(i18n.dedicatedSession)}</option>
                {currentSessionId &&
                  !sessions.some((session) => session.id === currentSessionId) && (
                    <option value={currentSessionId}>
                      {intl.formatMessage(i18n.currentSession)} — {currentSessionId}
                    </option>
                  )}
                {sessions.map((session) => (
                  <option key={session.id} value={session.id}>
                    {session.id === currentSessionId
                      ? `${intl.formatMessage(i18n.currentSession)} — ${session.name} — ${session.id}`
                      : `${session.name} — ${session.id}`}
                  </option>
                ))}
              </select>
              <Button onClick={() => void handlePair()} disabled={Boolean(busy)}>
                {intl.formatMessage(busy === 'pair' ? i18n.generating : i18n.generateCode)}
              </Button>

              {pairing && (
                <div className="rounded-md bg-background-secondary p-3">
                  <p className="text-xs text-text-secondary">
                    {intl.formatMessage(i18n.pairingCode)}
                  </p>
                  <div className="flex items-center gap-3 mt-1">
                    <code className="text-2xl font-semibold tracking-widest">{pairing.code}</code>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => void handleCopy('code', pairing.code)}
                    >
                      {copied === 'code' ? <Check /> : <Copy />}
                      {intl.formatMessage(copied === 'code' ? i18n.copied : i18n.copy)}
                    </Button>
                  </div>
                  <p className="text-xs text-text-secondary mt-2">
                    {intl.formatMessage(i18n.pairingExpiry, {
                      time: new Date(pairing.expiresAt * 1000).toLocaleTimeString(),
                    })}
                  </p>
                </div>
              )}
            </div>
          )}

          <div className="space-y-2">
            <p className="text-xs font-medium">{intl.formatMessage(i18n.pairedGroups)}</p>
            {gateway?.pairedUsers.length ? (
              gateway.pairedUsers.map((user) => (
                <div
                  key={user.userId}
                  className="flex items-center justify-between gap-3 rounded-md border p-3"
                >
                  <div className="min-w-0">
                    <p className="truncate text-xs font-medium">
                      {user.displayName ?? user.userId}
                    </p>
                    <p className="truncate font-mono text-[11px] text-text-secondary">
                      {user.userId}
                    </p>
                    <p className="truncate text-[11px] text-text-secondary">
                      {intl.formatMessage(i18n.pairedSession, { sessionId: user.sessionId })}
                    </p>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void handleUnpair(user.userId)}
                    disabled={Boolean(busy)}
                  >
                    <Unplug /> {intl.formatMessage(i18n.revoke)}
                  </Button>
                </div>
              ))
            ) : (
              <p className="text-xs text-text-secondary">
                {intl.formatMessage(i18n.noPairedGroups)}
              </p>
            )}
          </div>

          {error && (
            <p
              role="alert"
              className="rounded-md bg-red-50 p-3 text-xs text-red-700 dark:bg-red-950 dark:text-red-200"
            >
              {intl.formatMessage(i18n.error, { error })}
            </p>
          )}

          <div className="border-t pt-4 space-y-2">
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                onClick={() => void handleStop(false)}
                disabled={!running || Boolean(busy)}
              >
                <Unplug />
                {intl.formatMessage(busy === 'stop' ? i18n.stopping : i18n.stop)}
              </Button>
              <Button
                variant="destructive"
                onClick={() => void handleStop(true)}
                disabled={!gateway?.configured || Boolean(busy)}
              >
                <Trash2 /> {intl.formatMessage(i18n.forget)}
              </Button>
            </div>
            <p className="text-xs text-text-secondary">{intl.formatMessage(i18n.forgetHelp)}</p>
          </div>
        </CardContent>
      </Card>
    </section>
  );
}
