import { Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { BloodRequestEntity } from './entities/blood-request.entity';
import { BloodRequestItemEntity } from './entities/blood-request-item.entity';
import { BloodRequestsService } from './blood-requests.service';
import { BloodRequestsController } from './blood-requests.controller';
import { InventoryModule } from '../inventory/inventory.module';
import { BlockchainModule } from '../blockchain/blockchain.module';
import { NotificationsModule } from '../notifications/notifications.module';

@Module({
  imports: [
    TypeOrmModule.forFeature([BloodRequestEntity, BloodRequestItemEntity]),
    InventoryModule,
    BlockchainModule,
    NotificationsModule,
  ],
  controllers: [BloodRequestsController],
  providers: [BloodRequestsService],
  exports: [BloodRequestsService],
})
export class BloodRequestsModule {}
