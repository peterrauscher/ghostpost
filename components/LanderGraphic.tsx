import React from "react";
import { Animated, Easing, StyleSheet, View } from "react-native";

import Logo from "@/components/Logo";
import InstagramIcon from "@/assets/icons/Instagram.svg";
import TikTokIcon from "@/assets/icons/TikTok.svg";
import FacebookIcon from "@/assets/icons/Facebook.svg";
import XIcon from "@/assets/icons/X.svg";

type Props = {
  height: number;
};

export default function LanderGraphic({ height }: Props) {
  const bob = React.useRef(new Animated.Value(0)).current;
  const bobA = React.useRef(new Animated.Value(0)).current;
  const bobB = React.useRef(new Animated.Value(0)).current;
  const bobC = React.useRef(new Animated.Value(0)).current;
  const bobD = React.useRef(new Animated.Value(0)).current;
  const socialIconSize = 44;

  React.useEffect(() => {
    Animated.loop(
      Animated.sequence([
        Animated.timing(bob, {
          toValue: 1,
          duration: 1000,
          easing: Easing.inOut(Easing.ease),
          useNativeDriver: true,
        }),
        Animated.timing(bob, {
          toValue: 0,
          duration: 1000,
          easing: Easing.inOut(Easing.ease),
          useNativeDriver: true,
        }),
      ])
    ).start();

    const run = (val: Animated.Value, delay: number) => {
      const start = () =>
        Animated.loop(
          Animated.sequence([
            Animated.timing(val, {
              toValue: 1,
              duration: 1200,
              easing: Easing.inOut(Easing.ease),
              useNativeDriver: true,
            }),
            Animated.timing(val, {
              toValue: 0,
              duration: 1200,
              easing: Easing.inOut(Easing.ease),
              useNativeDriver: true,
            }),
          ])
        ).start();
      const id = setTimeout(start, delay);
      return () => clearTimeout(id);
    };
    const a = run(bobA, 0);
    const b = run(bobB, 200);
    const c = run(bobC, 400);
    const d = run(bobD, 600);
    return () => {
      a?.();
      b?.();
      c?.();
      d?.();
    };
  }, [bob, bobA, bobB, bobC, bobD]);

  const translateY = bob.interpolate({
    inputRange: [0, 1],
    outputRange: [0, -6],
  });

  return (
    <View style={[styles.wrap, { height }]}>
      {/* Floating platform logos */}
      <View pointerEvents="none" style={styles.floatsStage}>
        <Animated.View
          style={[
            styles.floatIcon,
            {
              left: 36,
              top: 0.35 * height,
              transform: [
                {
                  translateY: bobA.interpolate({
                    inputRange: [0, 1],
                    outputRange: [0, -8],
                  }),
                },
              ],
            },
          ]}
        >
          <InstagramIcon width={socialIconSize} height={socialIconSize} />
        </Animated.View>
        <Animated.View
          style={[
            styles.floatIcon,
            {
              right: 36,
              top: 0.25 * height,
              transform: [
                {
                  translateY: bobB.interpolate({
                    inputRange: [0, 1],
                    outputRange: [0, -6],
                  }),
                },
              ],
            },
          ]}
        >
          <TikTokIcon width={socialIconSize} height={socialIconSize} />
        </Animated.View>
        <Animated.View
          style={[
            styles.floatIcon,
            {
              left: 96,
              top: 0.1 * height,
              transform: [
                {
                  translateY: bobC.interpolate({
                    inputRange: [0, 1],
                    outputRange: [0, -7],
                  }),
                },
              ],
            },
          ]}
        >
          <FacebookIcon width={socialIconSize} height={socialIconSize} />
        </Animated.View>
        <Animated.View
          style={[
            styles.floatIcon,
            {
              right: 96,
              top: 0.08 * height,
              transform: [
                {
                  translateY: bobD.interpolate({
                    inputRange: [0, 1],
                    outputRange: [0, -5],
                  }),
                },
              ],
            },
          ]}
        >
          <XIcon width={socialIconSize} height={socialIconSize} />
        </Animated.View>
      </View>

      {/* Ghost */}
      <Animated.View
        style={[styles.ghostWrap, { transform: [{ translateY }] }]}
      >
        <Logo width={128} height={128} />
      </Animated.View>
    </View>
  );
}

const styles = StyleSheet.create({
  wrap: {
    width: "100%",
    backgroundColor: "transparent",
    alignItems: "center",
    justifyContent: "flex-end",
  },
  floatsStage: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    bottom: 0,
    backgroundColor: "transparent",
  },
  floatIcon: {
    position: "absolute",
    opacity: 0.9,
  },
  ghostWrap: {
    alignItems: "center",
    justifyContent: "center",
  },
});
