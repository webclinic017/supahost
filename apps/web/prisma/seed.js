import bcrypt from "bcryptjs";
import { PrismaClient } from "@prisma/client";

const prisma = new PrismaClient();

async function main() {
  const adminEmail = process.env.ADMIN_EMAIL;
  const adminPassword = process.env.ADMIN_PASSWORD;

  if (!adminEmail || !adminPassword) {
    console.log("ADMIN_EMAIL / ADMIN_PASSWORD not set; skipping admin seed.");
    return;
  }

  const existing = await prisma.user.findUnique({ where: { email: adminEmail } });
  if (existing) {
    if (existing.role !== "ADMIN") {
      await prisma.user.update({ where: { email: adminEmail }, data: { role: "ADMIN" } });
      console.log(`Updated ${adminEmail} to ADMIN`);
    } else {
      console.log(`Admin user already exists: ${adminEmail}`);
    }
    return;
  }

  const passwordHash = await bcrypt.hash(adminPassword, 10);
  await prisma.user.create({
    data: {
      email: adminEmail,
      passwordHash,
      role: "ADMIN",
      name: "Platform Admin"
    }
  });

  console.log(`Created admin user: ${adminEmail}`);
}

main()
  .catch((e) => {
    console.error(e);
    process.exit(1);
  })
  .finally(async () => {
    await prisma.$disconnect();
  });
