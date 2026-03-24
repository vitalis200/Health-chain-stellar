export type RiderStatus = 'available' | 'on_delivery' | 'offline' | 'suspended';

export interface Rider {
  id: string;
  name: string;
  phone: string;
  status: RiderStatus;
  currentZone: string;
  todayDeliveries: number;
  avgRating: number;
  lastActive: Date;
}

export interface RiderPerformance {
  date: string;
  deliveries: number;
  avgTime: number; // in minutes
  rating: number;
}

export interface RiderFilters {
  search: string;
  status: RiderStatus[];
}

export interface RiderSortConfig {
  column: keyof Rider;
  order: 'asc' | 'desc';
}