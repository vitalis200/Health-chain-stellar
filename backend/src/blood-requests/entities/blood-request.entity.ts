import {
  Entity,
  PrimaryGeneratedColumn,
  Column,
  CreateDateColumn,
  UpdateDateColumn,
  OneToMany,
} from 'typeorm';
import { BloodRequestStatus } from '../enums/blood-request-status.enum';
import { BloodRequestItemEntity } from './blood-request-item.entity';

@Entity('blood_requests')
export class BloodRequestEntity {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Column({ name: 'request_number', type: 'varchar', length: 64, unique: true })
  requestNumber: string;

  @Column({ name: 'hospital_id', type: 'varchar', length: 64 })
  hospitalId: string;

  @Column({ name: 'required_by', type: 'timestamptz' })
  requiredBy: Date;

  @Column({ name: 'delivery_address', type: 'text', nullable: true })
  deliveryAddress: string | null;

  @Column({ type: 'text', nullable: true })
  notes: string | null;

  @Column({
    type: 'varchar',
    length: 24,
    default: BloodRequestStatus.PENDING,
  })
  status: BloodRequestStatus;

  @Column({ name: 'blockchain_tx_hash', type: 'varchar', length: 256, nullable: true })
  blockchainTxHash: string | null;

  @Column({ name: 'created_by_user_id', type: 'varchar', length: 64, nullable: true })
  createdByUserId: string | null;

  @OneToMany(() => BloodRequestItemEntity, (item) => item.request, {
    cascade: true,
  })
  items: BloodRequestItemEntity[];

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;

  @UpdateDateColumn({ name: 'updated_at' })
  updatedAt: Date;
}
