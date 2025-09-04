// super tiny celebration: canvas-confetti fallback to alert
export async function celebrate() {
  try {
    const mod = await import("canvas-confetti");
    const confetti = mod.default;
    confetti({ particleCount: 160, spread: 80, scalar: 0.9 });
    setTimeout(() => confetti({ particleCount: 120, spread: 70 }), 300);
  } catch {
    alert("🎉 Block contributed!");
  }
}
