import { router } from 'expo-router';
import { StyleSheet, View } from 'react-native';
import { AppText, BackButton, Button, Screen } from '@/components';
import { useAuth } from '@/providers/auth-provider';
import { colors } from '@/theme';

export default function LoginScreen() {
  const auth = useAuth();
  return <Screen tone="welcome"><View style={styles.header}><BackButton onPress={() => router.back()} /></View><View style={styles.content}><AppText variant="title" align="center">log in</AppText><AppText variant="bodyRegular" color={colors.muted} align="center">Sign in to save your progress and upload your archive securely.</AppText>{auth.error ? <AppText variant="caption" color={colors.accentDeep} align="center">{auth.error}</AppText> : null}</View><View style={styles.footer}><Button label="continue securely →" onPress={() => void auth.signIn()} /></View></Screen>;
}
const styles = StyleSheet.create({ header: { paddingTop: 8 }, content: { flex: 1, justifyContent: 'center', gap: 14, paddingHorizontal: 8 }, footer: { paddingBottom: 12 } });
