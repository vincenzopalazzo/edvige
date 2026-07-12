import { beforeEach, describe, expect, it, vi } from 'vitest';
import { getAcpClient } from '../acpConnection';
import {
  getGatewayStatus,
  pairSonarGateway,
  startSonarGateway,
  stopSonarGateway,
  unpairSonarGateway,
} from '../gateways';

vi.mock('../acpConnection', () => ({
  getAcpClient: vi.fn(),
}));

function createClient() {
  return {
    goose: {
      gatewaysStatus_unstable: vi.fn(),
      gatewaysSonarStart_unstable: vi.fn(),
      gatewaysSonarPair_unstable: vi.fn(),
      gatewaysSonarStop_unstable: vi.fn(),
      gatewaysSonarUnpair_unstable: vi.fn(),
    },
  };
}

describe('ACP gateway helpers', () => {
  let client: ReturnType<typeof createClient>;

  beforeEach(() => {
    vi.clearAllMocks();
    client = createClient();
    vi.mocked(getAcpClient).mockResolvedValue(
      client as unknown as Awaited<ReturnType<typeof getAcpClient>>
    );
  });

  it('starts Sonar with explicit controllers and relays', async () => {
    const gateway = {
      gatewayType: 'sonar',
      running: true,
      configured: true,
      pairedUsers: [],
      info: { npub: 'npub1bridge' },
    };
    client.goose.gatewaysSonarStart_unstable.mockResolvedValue({ gateway });

    await expect(
      startSonarGateway(['npub1controller'], ['wss://nostr.relay.hedwig.sh/'])
    ).resolves.toEqual(gateway);
    expect(client.goose.gatewaysSonarStart_unstable).toHaveBeenCalledWith({
      controllers: ['npub1controller'],
      relays: ['wss://nostr.relay.hedwig.sh/'],
    });
  });

  it('returns status and creates a session-bound pairing code', async () => {
    client.goose.gatewaysStatus_unstable.mockResolvedValue({ gateways: [] });
    client.goose.gatewaysSonarPair_unstable.mockResolvedValue({
      code: 'ABC234',
      expiresAt: 123,
    });

    await expect(getGatewayStatus()).resolves.toEqual([]);
    await expect(pairSonarGateway('session-1')).resolves.toEqual({
      code: 'ABC234',
      expiresAt: 123,
    });
    expect(client.goose.gatewaysSonarPair_unstable).toHaveBeenCalledWith({
      sessionId: 'session-1',
    });
  });

  it('stops and optionally forgets Sonar configuration', async () => {
    await stopSonarGateway(true);
    expect(client.goose.gatewaysSonarStop_unstable).toHaveBeenCalledWith({ forget: true });
  });

  it('revokes a paired Sonar group', async () => {
    await unpairSonarGateway('group-1');
    expect(client.goose.gatewaysSonarUnpair_unstable).toHaveBeenCalledWith({
      groupId: 'group-1',
    });
  });
});
