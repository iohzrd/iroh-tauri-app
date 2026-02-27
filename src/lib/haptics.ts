export async function hapticImpact(
  style: "light" | "medium" | "heavy" = "medium",
) {
  try {
    const { impactFeedback } = await import("@tauri-apps/plugin-haptics");
    await impactFeedback(style);
  } catch {
    // Haptics not available on desktop
  }
}

export async function hapticNotification(
  type: "success" | "warning" | "error" = "success",
) {
  try {
    const { notificationFeedback } = await import("@tauri-apps/plugin-haptics");
    await notificationFeedback(type);
  } catch {
    // Haptics not available on desktop
  }
}
