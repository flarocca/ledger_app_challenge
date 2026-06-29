"use client";

import { useEffect } from "react";
import { useRouter } from "next/navigation";
import { api, MeResponse, ApiError } from "@/lib/api";

export default function Index() {
  const router = useRouter();
  useEffect(() => {
    api<MeResponse>("/users/me")
      .then(() => router.replace("/home"))
      .catch((err) => {
        if (err instanceof ApiError && err.status === 401) {
          router.replace("/login");
        } else {
          router.replace("/login");
        }
      });
  }, [router]);
  return null;
}
