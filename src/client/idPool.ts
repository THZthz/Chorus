let pool: string[] = [];
let fetching: Promise<void> | null = null;

async function refill(): Promise<void> {
  const res = await fetch("/api/ids/batch?count=20");
  if (!res.ok) throw new Error(`Failed to fetch IDs: ${res.status}`);
  const { ids } = await res.json();
  pool.push(...ids);
}

export async function nextId(): Promise<string> {
  if (pool.length < 5 && !fetching) {
    fetching = refill().finally(() => {
      fetching = null;
    });
  }
  if (pool.length === 0) {
    await fetching;
  }
  return pool.pop()!;
}

export async function initIdPool(): Promise<void> {
  await refill();
}
