import { Image } from 'expo-image';
import { View } from 'react-native';

export type MascotProp = 'calendar' | 'clipboard' | 'phone';

const ARTWORK = {
  calendar: require('@/assets/images/onboarding-calendar.png'),
  clipboard: require('@/assets/images/onboarding-clipboard.png'),
  phone: require('@/assets/images/onboarding-phone.png'),
};

export function OnboardingMascot({ prop, width = 190 }: { prop: MascotProp; width?: number }) {
  return (
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants" style={{ alignItems: 'center' }}>
      <Image source={ARTWORK[prop]} contentFit="contain" style={{ width, height: width * 328 / 380 }} />
    </View>
  );
}
