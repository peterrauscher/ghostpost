import type { ConcernOption, DashboardData, FlaggedPost, OnboardingAnswers, PlatformId, ReviewAction, ReviewListData, ScanStatus, UserProfile } from '@/domain/types';
import type { OnboardingState } from '@/domain/onboarding';

export type ClientKind = 'web' | 'native';
export interface AuthorizeResponse { authorizationUrl: string; state: string; expiresAt: string; exchangeSecret?: string }
export interface NativeSession { kind: 'bearer'; token: string; expiresAt: string }
export interface NativeExchangeResponse { user: UserProfile; session: NativeSession }
export interface PlatformCatalogItem { id: PlatformId; label: string; archiveEnabled: boolean; status?: string; glyph?: string; color?: string }
export interface PlatformCatalog { revision: string; platforms: PlatformCatalogItem[] }
export interface SignedPost { method: 'POST'; url: string; fields: Record<string, string> }
export interface ArchiveImport { id: string; platform: PlatformId; status: 'reserved'|'uploaded'|'queued'|'normalizing'|'ready'|'failed'|'deleted'|'cancelled'; errorCode?: string | null; upload?: SignedPost; expiresAt?: string }
export interface ArchiveImportList { items: ArchiveImport[]; nextCursor?: string | null }
export interface ReserveArchiveResponse { id: string; upload: SignedPost; expiresAt: string }
export interface Entitlement { status: string; productId: 'free_beta'; validFrom: string; expiresAt: string | null; scanId: string | null; capabilities: { reviewAccess: boolean; rescansRemaining: number | null; platformLimit: number } }
export interface ReviewListQuery { scanId?: string; risk?: 'all'|'high'|'medium'|'low'; status?: string; cursor?: string; limit?: number }

export interface GhostpostApi {
  authorize(client: ClientKind): Promise<AuthorizeResponse>;
  exchangeWeb(code: string, state: string): Promise<void>;
  exchangeNative(code: string, state: string, exchangeSecret: string): Promise<NativeExchangeResponse>;
  logout(): Promise<void>;
  getProfile(): Promise<UserProfile>;
  getOnboarding(): Promise<OnboardingState>;
  putOnboarding(state: OnboardingState, idempotencyKey: string): Promise<OnboardingState>;
  getPlatforms(): Promise<PlatformCatalog>;
  getEntitlement(): Promise<Entitlement>;
  listArchiveImports(): Promise<ArchiveImportList>;
  reserveArchiveImport(input: { platform: PlatformId; contentLength: number; contentType: string }, idempotencyKey: string): Promise<ReserveArchiveResponse>;
  completeArchiveImport(id: string, idempotencyKey: string): Promise<ArchiveImport>;
  getArchiveImport(id: string): Promise<ArchiveImport>;
  createScan(archiveImportIds: string[], idempotencyKey: string): Promise<ScanStatus>;
  getCurrentScan(): Promise<ScanStatus>;
  getScan(id: string): Promise<ScanStatus>;
  getDashboard(scanId?: string): Promise<DashboardData>;
  getReviewList(query?: ReviewListQuery): Promise<ReviewListData>;
  getFlaggedPost(id: string): Promise<FlaggedPost>;
  applyReviewAction(id: string, action: ReviewAction, expectedStatus: string, idempotencyKey: string): Promise<FlaggedPost>;
  deleteAccount(idempotencyKey: string): Promise<{ purgeDeadline?: string }>;
}

export type { ConcernOption, OnboardingAnswers };
