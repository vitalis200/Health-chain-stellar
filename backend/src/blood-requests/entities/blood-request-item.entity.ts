import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  ManyToOne,
  JoinColumn,
} from 'typeorm';
import { BloodRequestEntity } from './blood-request.entity';

@Entity('blood_request_items')
export class BloodRequestItemEntity {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ name: 'request_id', type: 'uuid' })
  requestId: string;

  @ManyToOne(() => BloodRequestEntity, (r) => r.items, { onDelete: 'CASCADE' })
  @JoinColumn({ name: 'request_id' })
  request: BloodRequestEntity;

  @Column({ name: 'blood_bank_id', type: 'varchar', length: 64 })
  bloodBankId: string;

  @Column({ name: 'blood_type', type: 'varchar', length: 16 })
  bloodType: string;

  @Column({ type: 'int' })
  quantity: number;
}
