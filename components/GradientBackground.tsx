import { ColorValue, StyleSheet } from "react-native";
import { LinearGradient } from "expo-linear-gradient";
import React from "react";

export function GradientBackground({
  // Brand gradient: Tekhelet → Medium Slate Blue @ ~45°
  colors = ["#3D348B", "#7678ED"] as readonly [ColorValue, ColorValue],
  start = { x: 0, y: 0 },
  end = { x: 1, y: 1 },
}: {
  colors?:
    | readonly [ColorValue, ColorValue]
    | readonly [ColorValue, ColorValue, ...ColorValue[]];
  start?: { x: number; y: number };
  end?: { x: number; y: number };
}) {
  return (
    <LinearGradient
      colors={colors}
      start={start}
      end={end}
      style={StyleSheet.absoluteFill}
    />
  );
}

export default GradientBackground;
