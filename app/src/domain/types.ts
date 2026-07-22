export type RiskLevel = 'high' | 'medium' | 'low';

export type PlatformId =
  | 'facebook'
  | 'reddit'
  | 'instagram'
  | 'tiktok'
  | 'x';

export type ComingUpOption =
  | 'rush'
  | 'college_apps'
  | 'job_interviews'
  | 'friends_family'
  | 'just_concerned'
  | 'something_else';

export type ConcernOption =
  | 'inappropriate_language'
  | 'drinking_drugs'
  | 'political_takes'
  | 'controversial_topics'
  | 'negativity'
  | 'public_image'
  | 'other';

export type ReviewAction = 'delete' | 'archive' | 'keep' | 'resolve';

export type ScanPhase = 'connecting' | 'scanning' | 'flagging' | 'complete';

export type AppGate =
  | 'welcome'
  | 'onboarding'
  | 'scan'
  | 'locked'
  | 'app';

export interface UserProfile {
  id: string;
  name: string;
  greetingName: string;
  avatarUrl?: string;
}

export interface OnboardingAnswers {
  comingUp: ComingUpOption[];
  concerns: ConcernOption[];
  platforms: PlatformId[];
}

export interface FocusArea {
  id: string;
  label: string;
  symbol: string;
}

export interface FlaggedPost {
  id: string;
  platform: PlatformId;
  platformLabel: string;
  date: string;
  quote: string;
  risk: RiskLevel;
  category: string;
  tags: string[];
  engagementLabel: string;
  likes: number;
  comments: number;
  explanation: string;
  whyFlagged: string;
  status: 'open' | 'resolved' | 'deleted' | 'archived' | 'kept';
}

export interface RiskSummary {
  level: RiskLevel;
  flaggedCount: number;
  gaugeSweep: number;
}

export interface DashboardData {
  user: UserProfile;
  auditHeadline: string;
  focusAreas: FocusArea[];
  flaggedPreview: FlaggedPost[];
  risk: RiskSummary;
  unlocked: boolean;
}

export interface ReviewFilters {
  all: number;
  high: number;
  medium: number;
  low: number;
}

export interface ReviewListData {
  posts: FlaggedPost[];
  filters: ReviewFilters;
}

export interface ScanStatus {
  phase: ScanPhase;
  progress: number;
  message: string;
}

export const COMING_UP_OPTIONS: { id: ComingUpOption; label: string }[] = [
  { id: 'rush', label: 'Rush' },
  { id: 'college_apps', label: 'College apps' },
  { id: 'job_interviews', label: 'Job interviews' },
  { id: 'friends_family', label: 'Friends or family' },
  { id: 'just_concerned', label: 'Just concerned' },
  { id: 'something_else', label: 'Something else' },
];

export const CONCERN_OPTIONS: { id: ConcernOption; label: string }[] = [
  { id: 'inappropriate_language', label: 'Inappropriate language' },
  { id: 'drinking_drugs', label: 'Drinking / drugs' },
  { id: 'political_takes', label: 'Political takes' },
  { id: 'controversial_topics', label: 'Controversial topics' },
  { id: 'negativity', label: 'Negativity' },
  { id: 'public_image', label: 'Public image' },
  { id: 'other', label: 'Other' },
];

export const PLATFORM_OPTIONS: {
  id: PlatformId;
  label: string;
  glyph: string;
  color: string;
  useImage?: boolean;
}[] = [
  { id: 'facebook', label: 'Facebook', glyph: 'f', color: '#1877F2' },
  { id: 'reddit', label: 'Reddit', glyph: '●', color: '#FF4500' },
  { id: 'instagram', label: 'Instagram', glyph: 'ig', color: '#E1306C', useImage: true },
  { id: 'tiktok', label: 'TikTok', glyph: '♪', color: '#111111', useImage: true },
  { id: 'x', label: 'X', glyph: '𝕏', color: '#111111', useImage: true },
];
