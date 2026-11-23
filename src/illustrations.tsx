// Simple original drawings for the "what does yours look like?" quiz. Schematic, not
// photos — enough to tell the options apart.

const pins = (x: number, y0: number, n: number, gap: number) =>
  Array.from({ length: n }, (_, i) => <circle key={`${x}-${i}`} cx={x} cy={y0 + i * gap} r={2.2} fill="#d4af37" />);

function ProMicroBoard({ color, label }: { color: string; label: string }) {
  return (
    <svg viewBox="0 0 90 160" width="54" height="96" aria-hidden="true">
      <rect x="27" y="2" width="36" height="14" rx="4" fill="#9ca3af" />
      <rect x="5" y="12" width="80" height="144" rx="6" fill={color} />
      {pins(14, 24, 12, 11)}
      {pins(76, 24, 12, 11)}
      <rect x="30" y="60" width="30" height="30" rx="2" fill="#111827" opacity=".85" />
      <text x="45" y="120" fill="#fff" fontSize="9" textAnchor="middle" fontFamily="sans-serif">{label}</text>
    </svg>
  );
}

function XiaoBoard() {
  return (
    <svg viewBox="0 0 90 100" width="54" height="60" aria-hidden="true">
      <rect x="30" y="2" width="30" height="12" rx="4" fill="#9ca3af" />
      <rect x="10" y="10" width="70" height="84" rx="5" fill="#1f2937" />
      <rect x="20" y="28" width="50" height="46" rx="3" fill="#cbd5e1" />
      <text x="45" y="56" fill="#334155" fontSize="10" textAnchor="middle" fontFamily="sans-serif">XIAO</text>
      {pins(14, 24, 7, 10)}
      {pins(76, 24, 7, 10)}
    </svg>
  );
}

function NiceViewScreen() {
  // Memory-in-pixel: paper-grey background, black pixels.
  return (
    <svg viewBox="0 0 170 78" width="136" height="62" aria-hidden="true">
      <rect x="1" y="1" width="168" height="76" rx="4" fill="#d6d3cb" stroke="#57534e" />
      <rect x="10" y="12" width="40" height="8" fill="#1c1917" />
      <rect x="10" y="28" width="26" height="26" fill="none" stroke="#1c1917" strokeWidth="3" />
      <path d="M70 60 L95 30 L110 48 L125 26 L160 60 Z" fill="#1c1917" />
      <circle cx="140" cy="18" r="6" fill="#1c1917" />
    </svg>
  );
}

function OledScreen() {
  return (
    <svg viewBox="0 0 170 50" width="136" height="40" aria-hidden="true">
      <rect x="1" y="1" width="168" height="48" rx="3" fill="#030712" stroke="#374151" />
      <rect x="12" y="12" width="70" height="6" rx="1" fill="#e0f2fe" />
      <rect x="12" y="26" width="110" height="6" rx="1" fill="#e0f2fe" opacity=".85" />
      <rect x="130" y="12" width="26" height="20" rx="2" fill="none" stroke="#e0f2fe" strokeWidth="2" />
    </svg>
  );
}

function NoScreen() {
  return (
    <svg viewBox="0 0 170 50" width="136" height="40" aria-hidden="true">
      <rect x="1" y="1" width="168" height="48" rx="3" fill="none" stroke="#94a3b8" strokeDasharray="6 4" />
      <text x="85" y="30" fill="#94a3b8" fontSize="12" textAnchor="middle" fontFamily="sans-serif">nothing here</text>
    </svg>
  );
}

export function Illustration({ value }: { value: string }) {
  switch (value) {
    case "nice_nano_v2": return <ProMicroBoard color="#6d28d9" label="nice!nano" />;
    case "nice_nano_v1": return <ProMicroBoard color="#4c1d95" label="nice!nano v1" />;
    case "nrf_micro": return <ProMicroBoard color="#047857" label="nRFMicro" />;
    case "xiao_ble": return <XiaoBoard />;
    case "nice_view": return <NiceViewScreen />;
    case "oled128x32": return <OledScreen />;
    case "none": return <NoScreen />;
    default: return null;
  }
}
