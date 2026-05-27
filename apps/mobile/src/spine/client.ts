import {
  clearCurrentSpineSession,
  getCurrentSpineSession,
} from "./session";

export async function revokeCurrentDevice(): Promise<void> {
  const session = await getCurrentSpineSession();
  if (!session) {
    return;
  }

  const response = await fetch(`${session.baseUrl}/v1/auth/revoke`, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${session.accessToken}`,
    },
  });

  if (!response.ok && response.status !== 401) {
    throw new Error(`Failed to revoke current device: ${response.status}`);
  }

  await clearCurrentSpineSession();
}

