import { api } from "./http-client";
import { Rider, RiderStatus } from "@/lib/types/riders";

const API_PREFIX = process.env.NEXT_PUBLIC_API_PREFIX || "api/v1";

export async function fetchRiders(): Promise<Rider[]> {
  const data = await api.get<Rider[]>(`/${API_PREFIX}/riders`);
  return data.map(rider => ({
    ...rider,
    lastActive: new Date(rider.lastActive)
  }));
}

export async function updateRiderStatus(id: string, status: RiderStatus, reason?: string) {
  return api.patch(`/${API_PREFIX}/riders/${id}/status`, { status, reason });
}

export async function assignRiderZone(id: string, zone: string) {
  return api.patch(`/${API_PREFIX}/riders/${id}/zone`, { zone });
}