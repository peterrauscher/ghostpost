/**
 * Ghostpost design tokens — sourced from Ghostpost Design System.pen variables.
 */

export const colors = {
  bg: '#F4EEFC',
  surface: '#FFFFFF',
  fg: '#171717',
  muted: '#6B6578',
  border: '#EDE8F5',
  accent: '#7F4AE0',
  accentDeep: '#5925BE',
  accentSoft: '#EDE4FF',
  screenTint: '#FAFAFC',
  black: '#141414',
  white: '#FFFFFF',
  riskHigh: '#E85A6B',
  riskHighText: '#B83245',
  riskHighBg: '#FDE8EB',
  riskMedium: '#F5A623',
  riskMediumText: '#8B5A00',
  riskMediumBg: '#FFF3DC',
  riskLow: '#3DB86E',
  riskLowText: '#247A45',
  riskLowBg: '#E6F7EC',
  gaugeTrack: '#F0EBF8',
  chevronMuted: '#C4BECF',
  tagBg: '#F5F3F8',
  tabInactive: '#B0AAB8',
  carouselInactive: '#D5C5F0',
  progressTrack: '#EBE1FA',
  overlay: '#F4EEFC47',
  shadowPurple: '#7F4AE047',
  shadowDark: '#0000002E',
  shadowSoft: '#3D24650A',
  shadowSheet: '#28145029',
  platformInstagram: '#E1306C',
  platformFacebook: '#1877F2',
  platformReddit: '#FF4500',
  platformTikTok: '#111111',
  platformX: '#111111',
  welcomeGradient: ['#F7F1FF', '#F0E6FF', '#EDE4FF'] as const,
  detailGradient: ['#FFF0F1', '#FFF7F6', '#FAFAFC'] as const,
  buttonGradient: ['#7F4AE0', '#5925BE'] as const,
  buttonDisabledGradient: ['#8F5AED', '#7F4AE0'] as const,
  auditGradient: ['#A78BFA', '#8B6AE8', '#7C5CF0'] as const,
  avatarGradient: ['#EDE4FF', '#D4C4FF'] as const,
  helpHeroGradient: ['#F8F3FF', '#EDE4FF', '#E4D6FF'] as const,
} as const;

export const fonts = {
  ui: 'SN Pro',
  body: 'SN Pro',
  mono: 'SN Pro',
} as const;

export const spacing = {
  0: 0,
  1: 2,
  2: 4,
  3: 6,
  4: 8,
  5: 10,
  6: 12,
  7: 14,
  8: 16,
  9: 18,
  10: 20,
  11: 22,
  12: 24,
  14: 28,
  16: 32,
  20: 40,
} as const;

export const radii = {
  sm: 6,
  md: 10,
  lg: 12,
  xl: 17,
  '2xl': 18,
  '3xl': 22,
  '4xl': 25,
  full: 999,
} as const;

export const typography = {
  display: { fontSize: 32, fontWeight: '700' as const, letterSpacing: -0.96, lineHeight: 38 },
  title: { fontSize: 24, fontWeight: '700' as const, letterSpacing: -0.72, lineHeight: 30 },
  titleSm: { fontSize: 22, fontWeight: '700' as const, letterSpacing: -0.4, lineHeight: 28 },
  headline: { fontSize: 18, fontWeight: '700' as const, letterSpacing: -0.36, lineHeight: 24 },
  bodyLg: { fontSize: 16, fontWeight: '600' as const, letterSpacing: -0.32, lineHeight: 22 },
  body: { fontSize: 15, fontWeight: '600' as const, letterSpacing: -0.3, lineHeight: 21 },
  bodyRegular: { fontSize: 13, fontWeight: '400' as const, letterSpacing: 0, lineHeight: 19 },
  caption: { fontSize: 12, fontWeight: '500' as const, letterSpacing: 0, lineHeight: 16 },
  captionBold: { fontSize: 12, fontWeight: '700' as const, letterSpacing: 0.24, lineHeight: 16 },
  label: { fontSize: 11, fontWeight: '600' as const, letterSpacing: 0, lineHeight: 14 },
  micro: { fontSize: 10, fontWeight: '600' as const, letterSpacing: 0.3, lineHeight: 12 },
} as const;

export const shadows = {
  button: {
    shadowColor: '#7F4AE0',
    shadowOffset: { width: 0, height: 6 },
    shadowOpacity: 0.28,
    shadowRadius: 18,
    elevation: 6,
  },
  darkButton: {
    shadowColor: '#000000',
    shadowOffset: { width: 0, height: 10 },
    shadowOpacity: 0.18,
    shadowRadius: 24,
    elevation: 8,
  },
  card: {
    shadowColor: '#3D2465',
    shadowOffset: { width: 0, height: 3 },
    shadowOpacity: 0.04,
    shadowRadius: 12,
    elevation: 2,
  },
  sheet: {
    shadowColor: '#281450',
    shadowOffset: { width: 0, height: 16 },
    shadowOpacity: 0.16,
    shadowRadius: 40,
    elevation: 12,
  },
} as const;

export const layout = {
  maxContentWidth: 480,
  screenPadding: 18,
  tabBarHeight: 64,
} as const;

export type RiskLevel = 'high' | 'medium' | 'low';

export const riskColors = {
  high: { text: colors.riskHighText, bg: colors.riskHighBg, dot: colors.riskHigh },
  medium: { text: colors.riskMediumText, bg: colors.riskMediumBg, dot: colors.riskMedium },
  low: { text: colors.riskLowText, bg: colors.riskLowBg, dot: colors.riskLow },
} as const;
