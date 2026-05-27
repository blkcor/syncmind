export interface SpineSession {
  baseUrl: string;
  accessToken: string;
}

let currentSession: SpineSession | null = null;

export async function getCurrentSpineSession(): Promise<SpineSession | null> {
  return currentSession;
}

export async function setCurrentSpineSession(session: SpineSession): Promise<void> {
  currentSession = { ...session };
}

export async function clearCurrentSpineSession(): Promise<void> {
  currentSession = null;
}

