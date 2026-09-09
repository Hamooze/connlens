import {
  siCloudflare,
  siDocker,
  siGithub,
  siGitlab,
  siGooglecloud,
  siNeon,
  siNetlify,
  siNpm,
  siSentry,
  siShopify,
  siStripe,
  siSupabase,
  siVercel,
} from "simple-icons";
import type { SimpleIcon } from "simple-icons";
interface ProviderVisual {
  name: string;
  color: string;
  icon?: SimpleIcon;
  monogram?: string;
}

const providerVisuals: Record<string, ProviderVisual> = {
  github: { name: "GitHub", color: "#f5f5f5", icon: siGithub },
  gitlab: { name: "GitLab", color: "#fc6d26", icon: siGitlab },
  aws: { name: "AWS", color: "#ff9900", monogram: "AWS" },
  azure: { name: "Azure", color: "#50a7ff", monogram: "AZ" },
  vercel: { name: "Vercel", color: "#ffffff", icon: siVercel },
  neon: { name: "Neon DB", color: "#00e599", icon: siNeon },
  docker: { name: "Docker", color: "#2496ed", icon: siDocker },
  npm: { name: "npm", color: "#cb3837", icon: siNpm },
  gcloud: { name: "Google Cloud", color: "#4285f4", icon: siGooglecloud },
  netlify: { name: "Netlify", color: "#00c7b7", icon: siNetlify },
  cloudflare: { name: "Cloudflare", color: "#f38020", icon: siCloudflare },
  stripe: { name: "Stripe", color: "#635bff", icon: siStripe },
  shopify: { name: "Shopify", color: "#95bf47", icon: siShopify },
  supabase: { name: "Supabase", color: "#3ecf8e", icon: siSupabase },
  sentry: { name: "Sentry", color: "#fb4226", icon: siSentry },
};

export const popularProviderIds = [
  "github",
  "aws",
  "vercel",
  "docker",
  "npm",
  "neon",
  "gitlab",
  "gcloud",
  "azure",
  "netlify",
  "cloudflare",
  "stripe",
  "supabase",
  "shopify",
  "sentry",
];

export function ProviderLogo({ provider }: { provider: string }) {
  const visual = providerVisuals[provider] ?? {
    name: provider,
    color: "#d7d7d7",
    monogram: provider.slice(0, 2).toUpperCase(),
  };

  return (
    <span className="provider-logo" style={{ color: visual.color }} aria-hidden="true">
      {visual.icon ? (
        <svg viewBox="0 0 24 24" role="img">
          <path d={visual.icon.path} />
        </svg>
      ) : (
        <span>{visual.monogram}</span>
      )}
    </span>
  );
}

export function displayProviderName(provider: string, fallback: string) {
  return providerVisuals[provider]?.name ?? fallback;
}


export function providerName(id: string) { return providerVisuals[id]?.name ?? id; }
