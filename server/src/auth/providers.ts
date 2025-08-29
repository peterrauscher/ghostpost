import { Facebook, Twitter } from "arctic";

export type ProviderConfig = {
  facebookClientId: string;
  facebookClientSecret: string;
  facebookRedirectUri: string;
  twitterClientId: string;
  twitterClientSecret: string;
  twitterRedirectUri: string;
};

export function createProviders(cfg: ProviderConfig) {
  const facebook = new Facebook(
    cfg.facebookClientId,
    cfg.facebookClientSecret,
    cfg.facebookRedirectUri
  );
  const twitter = new Twitter(
    cfg.twitterClientId,
    cfg.twitterClientSecret,
    cfg.twitterRedirectUri
  );
  return { facebook, twitter };
}
