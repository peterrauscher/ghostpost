import Colors from "@/constants/Colors";

export const spacing = {
  xxs: 4,
  xs: 8,
  sm: 12,
  md: 16,
  lg: 24,
  xl: 32,
  xxl: 40,
} as const;

export const radii = {
  sm: 8,
  md: 12,
  lg: 16,
  xl: 24,
  pill: 999,
} as const;

export const fontFamilies = {
  regular: "Inter_400Regular",
  semibold: "Inter_600SemiBold",
  bold: "Inter_700Bold",
  mono: "SpaceMono",
} as const;

export const fontSizes = {
  xs: 12,
  sm: 14,
  md: 16,
  lg: 20,
  xl: 24,
  xxl: 30,
  display: 48,
} as const;

export const theme = (scheme: "light" | "dark" = "light") => Colors[scheme];

export type Spacing = typeof spacing;
export type Radii = typeof radii;
export type FontFamilies = typeof fontFamilies;
export type FontSizes = typeof fontSizes;
