export interface OutboxItem {
  id: string;
  payload: unknown;
}

let outboxItems: OutboxItem[] = [];

export async function enqueueOutboxItem(item: OutboxItem): Promise<void> {
  outboxItems = [...outboxItems, item];
}

export async function getOutboxItems(): Promise<OutboxItem[]> {
  return outboxItems.map((item) => ({ ...item }));
}

export async function clearOutbox(): Promise<void> {
  outboxItems = [];
}

