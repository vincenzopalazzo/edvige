import type { GatewayStatusDto, PairSonarGatewayResponse_unstable } from '@aaif/goose-sdk';
import { getAcpClient } from './acpConnection';

export async function getGatewayStatus(): Promise<GatewayStatusDto[]> {
  const client = await getAcpClient();
  const response = await client.goose.gatewaysStatus_unstable({});
  return response.gateways;
}

export async function startSonarGateway(
  controllers: string[],
  relays: string[]
): Promise<GatewayStatusDto> {
  const client = await getAcpClient();
  const response = await client.goose.gatewaysSonarStart_unstable({ controllers, relays });
  return response.gateway;
}

export async function pairSonarGateway(
  sessionId?: string
): Promise<PairSonarGatewayResponse_unstable> {
  const client = await getAcpClient();
  return client.goose.gatewaysSonarPair_unstable({ sessionId });
}

export async function stopSonarGateway(forget = false): Promise<void> {
  const client = await getAcpClient();
  await client.goose.gatewaysSonarStop_unstable({ forget });
}

export async function unpairSonarGateway(groupId: string): Promise<void> {
  const client = await getAcpClient();
  await client.goose.gatewaysSonarUnpair_unstable({ groupId });
}
