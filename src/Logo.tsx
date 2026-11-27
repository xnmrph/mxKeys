import { useId } from "react";

/**
 * The mxKeys logo, "typed out": a wider mx key (blackletter, with a yellow homing bar like
 * the bumps on F and J) followed by K E Y S keycaps. Same keycap look as the app icon
 * (tools/icon/icon.html); the banner export is drawn by tools/icon/banner.html.
 */
export function Logo({ width = 188, className }: { width?: number; className?: string }) {
  const id = useId().replace(/:/g, "");
  const cap = `cap-${id}`, top = `top-${id}`, sh = `sh-${id}`;
  const Key = ({ x, w, children }: { x: number; w: number; children: React.ReactNode }) => (
    <g transform={`translate(${x} 0)`}>
      <g filter={`url(#${sh})`}>
        <rect width={w} height={232} rx={40} fill={`url(#${cap})`} stroke="#3A4754" strokeWidth={3} />
        <rect x={w > 300 ? 26 : 22} y={12} width={w - (w > 300 ? 52 : 44)} height={186} rx={30} fill={`url(#${top})`} stroke="#3E4B59" strokeWidth={2} />
      </g>
      {children}
    </g>
  );
  return (
    <svg className={className} width={width} height={(width * 256) / 1440} viewBox="-14 -6 1440 256" role="img" aria-label="mxKeys">
      <defs>
        <linearGradient id={cap} x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#2E3A47" /><stop offset="1" stopColor="#18212B" /></linearGradient>
        <linearGradient id={top} x1="0" y1="0" x2="0" y2="1"><stop offset="0" stopColor="#2A3541" /><stop offset="1" stopColor="#1F2934" /></linearGradient>
        <filter id={sh} x="-20%" y="-20%" width="140%" height="150%"><feDropShadow dx="0" dy="8" stdDeviation="7" floodColor="#000" floodOpacity=".4" /></filter>
      </defs>
      <Key x={0} w={356}>
        <text x={178} y={136} textAnchor="middle" fontFamily='"Grenze Gotisch", serif' fontWeight={900} fontSize={148} fill="#EEE9E0">mx</text>
        <rect x={148} y={174} width={60} height={8} rx={4} fill="#FFE714" />
      </Key>
      {["K", "E", "Y", "S"].map((l, i) => (
        <Key key={l} x={388 + i * 264} w={232}>
          <text x={116} y={146} textAnchor="middle" fontFamily='"IBM Plex Sans", system-ui, sans-serif' fontWeight={600} fontSize={112} fill="#C9D1D8">{l}</text>
        </Key>
      ))}
    </svg>
  );
}
