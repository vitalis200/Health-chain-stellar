import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { BullModule } from '@nestjs/bullmq';
import { ScheduleModule } from '@nestjs/schedule';
import { EventEmitterModule } from '@nestjs/event-emitter';

import { InventoryService } from './inventory.service';
import { InventoryController } from './inventory.controller';
import { InventoryForecastingService } from './inventory-forecasting.service';
import { InventoryEventListener } from './inventory-event.listener';
import { DonorOutreachProcessor } from './processors/donor-outreach.processor';
import { OrderEntity } from '../orders/entities/order.entity';
import { InventoryEntity } from './entities/inventory.entity';
import { NotificationsModule } from '../notifications/notifications.module';
import { UsersModule } from '../users/users.module';

@Module({
  imports: [
    TypeOrmModule.forFeature([OrderEntity, InventoryEntity]),
    BullModule.registerQueue({
      name: 'donor-outreach',
    }),
    ScheduleModule.forRoot(),
    EventEmitterModule.forRoot(),
    NotificationsModule,
    UsersModule,
  ],
  controllers: [InventoryController],
  providers: [
    InventoryService,
    InventoryForecastingService,
    InventoryEventListener,
    DonorOutreachProcessor,
  ],
  exports: [InventoryService, InventoryForecastingService],
})
export class InventoryModule { }
