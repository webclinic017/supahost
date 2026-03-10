import Link from "next/link";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";

const plans = [
  {
    id: "starter",
    name: "Starter",
    price: "$25/mo",
    desc: "For small apps and dev projects.",
    bullets: ["1 Supabase project", "Basic rate limits", "Community support"]
  },
  {
    id: "pro",
    name: "Pro",
    price: "$99/mo",
    desc: "For production workloads.",
    bullets: ["Multiple projects", "Higher limits", "Priority support"]
  }
];

export default function PricingPage() {
  return (
    <div className="space-y-8">
      <div>
        <h1 className="text-2xl font-bold">Pricing</h1>
        <p className="text-slate-600 dark:text-slate-300">
          Plans are mapped to Stripe Price IDs via environment variables.
        </p>
      </div>

      <div className="grid md:grid-cols-2 gap-6">
        {plans.map((p) => (
          <Card key={p.id}>
            <CardHeader>
              <CardTitle>{p.name}</CardTitle>
              <CardDescription>{p.desc}</CardDescription>
            </CardHeader>
            <CardContent>
              <div className="text-3xl font-bold">{p.price}</div>
              <ul className="mt-4 list-disc pl-5 text-sm text-slate-600 dark:text-slate-300">
                {p.bullets.map((b) => (
                  <li key={b}>{b}</li>
                ))}
              </ul>
            </CardContent>
            <CardFooter>
              <Link href={`/signup?plan=${p.id}`}>
                <Button>Choose {p.name}</Button>
              </Link>
            </CardFooter>
          </Card>
        ))}
      </div>
    </div>
  );
}
