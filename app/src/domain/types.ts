export type RiskLevel = 'high' | 'medium' | 'low';
export type OverallRiskLevel = RiskLevel | 'none';
export type PlatformId = 'facebook' | 'reddit' | 'instagram' | 'tiktok' | 'x';
export type ComingUpOption = 'rush'|'college_apps'|'job_interviews'|'friends_family'|'just_concerned'|'something_else'|'internships'|'career_change'|'background_check';
export type ConcernOption = 'inappropriate_language'|'underage_drinking'|'drug_use'|'hate'|'political_takes'|'controversial_topics'|'negativity'|'public_image';
export type ReviewAction = 'delete_local' | 'archive' | 'keep' | 'resolve';
export type ScanPhase = 'connecting' | 'scanning' | 'flagging' | 'complete';
export type ScanState = 'queued' | 'running' | 'succeeded' | 'failed' | 'cancelled';

export interface UserProfile { id: string; name: string; greetingName: string; avatarUrl?: string | null; email?: string }
export interface OnboardingAnswers { comingUp: ComingUpOption[]; concerns: ConcernOption[]; platforms: PlatformId[]; disclosureConsent: { accepted: boolean; version: string } }
export interface FocusArea { id: string; label: string; symbol: string }
export interface FlaggedPost {
  id: string; platform: PlatformId; platformLabel: string; date: string; quote: string;
  risk: RiskLevel; category: string; tags: string[]; engagementLabel?: string; likes?: number;
  comments?: number; explanation: string; whyFlagged: string;
  status: 'open' | 'resolved' | 'deleted' | 'archived' | 'kept';
}
export interface RiskSummary { level: OverallRiskLevel; flaggedCount: number; gaugeSweep: number }
export interface DashboardData { user: UserProfile; auditHeadline: string; focusAreas: FocusArea[]; flaggedPreview: FlaggedPost[]; risk: RiskSummary }
export interface ReviewFilters { all: number; high: number; medium: number; low: number }
export interface ReviewListData { posts: FlaggedPost[]; filters: ReviewFilters; nextCursor?: string | null; hasMore?: boolean }
export interface ScanStatus {
  id: string; status: ScanState; phase: ScanPhase; progress: number; message: string;
  errorCode?: string | null; createdAt?: string; finishedAt?: string | null;
}

export const COMING_UP_OPTIONS: { id: ComingUpOption; label: string }[] = [
  { id: 'rush', label: 'Rush' }, { id: 'college_apps', label: 'College applications' },
  { id: 'job_interviews', label: 'Job interviews' }, { id: 'friends_family', label: 'Social or family events' },
  { id: 'just_concerned', label: 'Nothing specific yet' }, { id: 'something_else', label: 'Another upcoming event' },
  { id: 'internships', label: 'Internships' }, { id: 'career_change', label: 'A new job or career move' },
  { id: 'background_check', label: 'A background check' },
];
export const CONCERN_OPTIONS: { id: ConcernOption; label: string }[] = [
  { id: 'inappropriate_language', label: 'Inappropriate language' }, { id: 'underage_drinking', label: 'Underage drinking' },
  { id: 'drug_use', label: 'Drug use' }, { id: 'hate', label: 'Hate or harassment' },
  { id: 'political_takes', label: 'Political posts' }, { id: 'controversial_topics', label: 'Controversial posts' },
  { id: 'negativity', label: 'Negative posts' }, { id: 'public_image', label: 'Risky or unprofessional behavior' },
];
export const PLATFORM_OPTIONS = [
  { id: 'facebook', label: 'Facebook', glyph: 'f', color: '#0866FF' },
  { id: 'reddit', label: 'Reddit', glyph: '●', color: '#FF4500' },
  { id: 'instagram', label: 'Instagram', glyph: 'ig', color: '#E1306C', useImage: true },
  { id: 'tiktok', label: 'TikTok', glyph: '♪', color: '#111111', useImage: true },
  { id: 'x', label: 'X', glyph: '𝕏', color: '#111111', useImage: true },
] satisfies { id: PlatformId; label: string; glyph: string; color: string; useImage?: boolean }[];
