import {
  MigrationInterface,
  QueryRunner,
  Table,
  TableForeignKey,
  TableIndex,
} from 'typeorm';

export class CreateBloodRequestsTables1772000000000 implements MigrationInterface {
  public async up(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.createTable(
      new Table({
        name: 'blood_requests',
        columns: [
          {
            name: 'id',
            type: 'uuid',
            isPrimary: true,
            generationStrategy: 'uuid',
            default: 'uuid_generate_v4()',
          },
          {
            name: 'request_number',
            type: 'varchar',
            length: '64',
            isUnique: true,
          },
          { name: 'hospital_id', type: 'varchar', length: '64' },
          { name: 'required_by', type: 'timestamptz' },
          { name: 'delivery_address', type: 'text', isNullable: true },
          { name: 'notes', type: 'text', isNullable: true },
          {
            name: 'status',
            type: 'varchar',
            length: '24',
            default: `'pending'`,
          },
          { name: 'blockchain_tx_hash', type: 'varchar', length: '256', isNullable: true },
          { name: 'created_by_user_id', type: 'varchar', length: '64', isNullable: true },
          { name: 'created_at', type: 'timestamp', default: 'now()' },
          { name: 'updated_at', type: 'timestamp', default: 'now()' },
        ],
      }),
      true,
    );

    await queryRunner.createTable(
      new Table({
        name: 'blood_request_items',
        columns: [
          {
            name: 'id',
            type: 'uuid',
            isPrimary: true,
            generationStrategy: 'uuid',
            default: 'uuid_generate_v4()',
          },
          { name: 'request_id', type: 'uuid' },
          { name: 'blood_bank_id', type: 'varchar', length: '64' },
          { name: 'blood_type', type: 'varchar', length: '16' },
          { name: 'quantity', type: 'int' },
        ],
      }),
      true,
    );

    await queryRunner.createForeignKey(
      'blood_request_items',
      new TableForeignKey({
        name: 'FK_blood_request_items_request',
        columnNames: ['request_id'],
        referencedTableName: 'blood_requests',
        referencedColumnNames: ['id'],
        onDelete: 'CASCADE',
      }),
    );

    await queryRunner.createIndex(
      'blood_request_items',
      new TableIndex({
        name: 'IDX_blood_request_items_request_id',
        columnNames: ['request_id'],
      }),
    );
  }

  public async down(queryRunner: QueryRunner): Promise<void> {
    await queryRunner.dropForeignKey(
      'blood_request_items',
      'FK_blood_request_items_request',
    );
    await queryRunner.dropTable('blood_request_items', true);
    await queryRunner.dropTable('blood_requests', true);
  }
}
