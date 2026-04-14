interface LayersLogoProps {
  size?: number;
}

export function LayersLogo({ size = 28 }: LayersLogoProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
    >
      <path
        d="M16 4L3 11L16 18L29 11L16 4Z"
        fill="currentColor"
        opacity="0.9"
      />
      <path
        d="M3 16L16 23L29 16"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.6"
      />
      <path
        d="M3 21L16 28L29 21"
        stroke="currentColor"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
        opacity="0.35"
      />
    </svg>
  );
}
