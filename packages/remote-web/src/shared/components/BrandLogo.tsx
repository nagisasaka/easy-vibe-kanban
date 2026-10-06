interface BrandLogoProps {
  className?: string;
  alt?: string;
}

export function BrandLogo({
  className = "h-8 w-auto",
  alt = "Lucky Vibe Kanban",
}: BrandLogoProps) {
  return (
    <picture>
      <source
        srcSet="/lvk-logo-dark.svg"
        media="(prefers-color-scheme: dark)"
      />
      <img src="/lvk-logo.svg" alt={alt} className={className} />
    </picture>
  );
}
