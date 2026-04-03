// GhostPost branding palette
// Primary – Tekhelet: #3D348B
// Secondary – Medium Slate Blue: #7678ED
// Success – Fern Green: #4C7240
// Text – Jet: #303030
// Background – Baby Powder: #FBF9F4
// Warn – Selective Yellow: #F7B801
// Danger – Persimmon: #F35B04
// Critical – Scarlet: #F5381B

const TEKHELET = "#3D348B";
const MEDIUM_SLATE_BLUE = "#7678ED";
const FERN_GREEN = "#4C7240";
const JET = "#303030";
const BABY_POWDER = "#FBF9F4";
const SELECTIVE_YELLOW = "#F7B801";
const PERSIMMON = "#F35B04";
const SCARLET = "#F5381B";

export default {
  light: {
    text: JET,
    background: BABY_POWDER,
    tint: TEKHELET,
    primary: TEKHELET,
    secondary: MEDIUM_SLATE_BLUE,
    success: FERN_GREEN,
    warn: SELECTIVE_YELLOW,
    danger: PERSIMMON,
    critical: SCARLET,
    tabIconDefault: "#B0B0B0",
    tabIconSelected: TEKHELET,
  },
  dark: {
    text: "#FFFFFF",
    // Dark surface uses a subtle Tekhelet overlay (~8%) over black
    background: "#05040B",
    tint: TEKHELET,
    primary: TEKHELET,
    secondary: MEDIUM_SLATE_BLUE,
    success: FERN_GREEN,
    warn: SELECTIVE_YELLOW,
    danger: PERSIMMON,
    critical: SCARLET,
    tabIconDefault: "#777777",
    tabIconSelected: TEKHELET,
  },
};
