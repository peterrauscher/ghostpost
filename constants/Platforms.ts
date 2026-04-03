import InstagramIcon from "@/assets/icons/Instagram.svg";
import TikTokIcon from "@/assets/icons/TikTok.svg";
import FacebookIcon from "@/assets/icons/Facebook.svg";
import XIcon from "@/assets/icons/X.svg";
import ThreadsIcon from "@/assets/icons/Threads.svg";
import LinkedInIcon from "@/assets/icons/LinkedIn.svg";
import type { SvgProps } from "react-native-svg";

export const PLATFORM_KEYS = [
  "tiktok",
  "instagram",
  "twitter",
  "threads",
  "facebook",
  "linkedin",
] as const;

export type Platform = (typeof PLATFORM_KEYS)[number];

export type PlatformMeta = {
  key: Platform;
  label: string;
  Icon: React.FC<SvgProps>;
};

export const PLATFORM_ICON: Record<Platform, React.FC<SvgProps>> = {
  tiktok: TikTokIcon,
  instagram: InstagramIcon,
  twitter: XIcon,
  threads: ThreadsIcon,
  facebook: FacebookIcon,
  linkedin: LinkedInIcon,
};

export const PLATFORM_LABEL: Record<Platform, string> = {
  tiktok: "TikTok",
  instagram: "Instagram",
  twitter: "X (Formerly Twitter)",
  threads: "Threads",
  facebook: "Facebook",
  linkedin: "LinkedIn",
};

export const PLATFORMS: PlatformMeta[] = PLATFORM_KEYS.map((key) => ({
  key,
  label: PLATFORM_LABEL[key],
  Icon: PLATFORM_ICON[key],
}));

// Backend-supported providers (DB enum)
export const BACKEND_PROVIDER_KEYS = [
  "instagram",
  "tiktok",
  "twitter",
  "facebook",
] as const;

export type BackendProvider = (typeof BACKEND_PROVIDER_KEYS)[number];

export function isBackendProvider(p: Platform): p is BackendProvider {
  return (BACKEND_PROVIDER_KEYS as readonly string[]).includes(p);
}

export const Platforms = {
  TIKTOK: "tiktok",
  INSTAGRAM: "instagram",
  TWITTER: "twitter",
  THREADS: "threads",
  FACEBOOK: "facebook",
  LINKEDIN: "linkedin",
} as const;

export default Platforms;
