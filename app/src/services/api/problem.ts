export interface ProblemDetails { type?: string; title?: string; status?: number; detail?: string; instance?: string; code?: string; [key: string]: unknown }

export class ApiError extends Error {
  constructor(public readonly status: number, message: string) { super(message); this.name = 'ApiError'; }
}

export class ProblemError extends ApiError {
  constructor(public readonly problem: ProblemDetails) {
    super(problem.status ?? 0, problem.detail ?? problem.title ?? 'Request failed');
    this.name = 'ProblemError';
  }
  get code() { return typeof this.problem.code === 'string' ? this.problem.code : undefined; }
}

export async function errorFromResponse(response: Response): Promise<ApiError> {
  const contentType = response.headers.get('content-type') ?? '';
  if (contentType.toLowerCase().includes('application/problem+json')) {
    try { return new ProblemError(await response.json() as ProblemDetails); } catch { /* generic fallback */ }
  }
  const text = await response.text().catch(() => '');
  return new ApiError(response.status, text || response.statusText || `Request failed: ${response.status}`);
}
