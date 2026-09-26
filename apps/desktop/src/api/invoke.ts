let sessionToken: Promise<string> | null = null;

async function token(): Promise<string> {
  sessionToken ??= fetch("/api/session", { credentials: "same-origin", cache: "no-store" })
    .then(async (response) => {
      if (!response.ok) throw new Error("Local service is unavailable");
      const body = await response.json() as { token: string };
      return body.token;
    }).catch((error) => { sessionToken = null; throw error; });
  return sessionToken;
}

export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const request = async (secret: string) => fetch("/api/invoke", {
    method: "POST",
    headers: { "Content-Type": "application/json", "X-MAS-Token": secret },
    body: JSON.stringify({ command, args }),
    credentials: "same-origin",
    cache: "no-store",
  });
  let response = await request(await token());
  if (response.status === 401) {
    sessionToken = null;
    response = await request(await token());
  }
  const body = await response.json().catch(() => null);
  if (!response.ok) throw body?.error ?? new Error(
    response.status === 413 ? "The workspace bundle exceeds the local import limit." : `Local service returned ${response.status}`,
  );
  return body as T;
}
