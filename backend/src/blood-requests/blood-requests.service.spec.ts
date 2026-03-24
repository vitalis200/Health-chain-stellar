/// <reference types="jest" />
import { Test, TestingModule } from '@nestjs/testing';
import { getRepositoryToken } from '@nestjs/typeorm';
import {
  BadRequestException,
  ForbiddenException,
  UnprocessableEntityException,
} from '@nestjs/common';
import { BloodRequestsService } from './blood-requests.service';
import { BloodRequestEntity } from './entities/blood-request.entity';
import { BloodRequestItemEntity } from './entities/blood-request-item.entity';
import { InventoryService } from '../inventory/inventory.service';
import { SorobanService } from '../blockchain/services/soroban.service';
import { EmailProvider } from '../notifications/providers/email.provider';
import { UserRole } from '../auth/enums/user-role.enum';

describe('BloodRequestsService', () => {
  let service: BloodRequestsService;
  let bloodRequestRepo: { create: jest.Mock; save: jest.Mock; exist: jest.Mock };
  let bloodRequestItemRepo: { create: jest.Mock };
  let inventory: { reserveStockOrThrow: jest.Mock; releaseStockByBankAndType: jest.Mock };
  let soroban: { submitTransactionAndWait: jest.Mock };
  let email: { send: jest.Mock };

  const futureIso = () => new Date(Date.now() + 86_400_000).toISOString();

  beforeEach(async () => {
    bloodRequestRepo = {
      create: jest.fn((x) => ({ ...x })),
      save: jest.fn(async (e) => ({
        ...e,
        id: '11111111-1111-1111-1111-111111111111',
        items: (e.items || []).map((i: object, idx: number) => ({
          ...i,
          id: `item-${idx}`,
        })),
      })),
      exist: jest.fn().mockResolvedValue(false),
    };
    bloodRequestItemRepo = {
      create: jest.fn((x) => ({ ...x })),
    };
    inventory = {
      reserveStockOrThrow: jest.fn().mockResolvedValue(undefined),
      releaseStockByBankAndType: jest.fn().mockResolvedValue(undefined),
    };
    soroban = {
      submitTransactionAndWait: jest
        .fn()
        .mockResolvedValue({ transactionHash: 'tx_blood_req' }),
    };
    email = { send: jest.fn().mockResolvedValue(undefined) };

    const module: TestingModule = await Test.createTestingModule({
      providers: [
        BloodRequestsService,
        { provide: getRepositoryToken(BloodRequestEntity), useValue: bloodRequestRepo },
        {
          provide: getRepositoryToken(BloodRequestItemEntity),
          useValue: bloodRequestItemRepo,
        },
        { provide: InventoryService, useValue: inventory },
        { provide: SorobanService, useValue: soroban },
        { provide: EmailProvider, useValue: email },
      ],
    }).compile();

    service = module.get(BloodRequestsService);
  });

  it('rejects hospital user creating for another hospitalId', async () => {
    await expect(
      service.create(
        {
          hospitalId: 'other-hospital',
          requiredBy: futureIso(),
          items: [
            { bloodType: 'O+', quantity: 1, bloodBankId: 'bank-1' },
          ],
        },
        { id: 'my-hospital', role: UserRole.HOSPITAL, email: 'h@x.com' },
      ),
    ).rejects.toBeInstanceOf(ForbiddenException);
    expect(inventory.reserveStockOrThrow).not.toHaveBeenCalled();
  });

  it('rejects requiredBy in the past', async () => {
    await expect(
      service.create(
        {
          hospitalId: 'h1',
          requiredBy: new Date(Date.now() - 1000).toISOString(),
          items: [{ bloodType: 'O+', quantity: 1, bloodBankId: 'bank-1' }],
        },
        { id: 'h1', role: UserRole.HOSPITAL, email: 'h@x.com' },
      ),
    ).rejects.toBeInstanceOf(BadRequestException);
  });

  it('creates multi-item request, reserves stock, chain, email', async () => {
    const res = await service.create(
      {
        hospitalId: 'h1',
        requiredBy: futureIso(),
        items: [
          { bloodType: 'O+', quantity: 2, bloodBankId: 'bank-a' },
          { bloodType: 'A-', quantity: 1, bloodBankId: 'bank-b' },
        ],
        deliveryAddress: 'Ward 4',
      },
      { id: 'h1', role: UserRole.HOSPITAL, email: 'hospital@test.com' },
    );

    expect(inventory.reserveStockOrThrow).toHaveBeenCalledTimes(2);
    expect(soroban.submitTransactionAndWait).toHaveBeenCalledWith(
      expect.objectContaining({
        contractMethod: 'create_blood_request',
        idempotencyKey: expect.stringMatching(/^blood-request:BR-/),
      }),
    );
    expect(res.data.blockchainTxHash).toBe('tx_blood_req');
    expect(res.data.items).toHaveLength(2);
    expect(email.send).toHaveBeenCalledWith(
      'hospital@test.com',
      expect.any(String),
      expect.stringContaining('BR-'),
    );
  });

  it('releases inventory when Soroban fails', async () => {
    soroban.submitTransactionAndWait.mockRejectedValueOnce(new Error('chain down'));

    await expect(
      service.create(
        {
          hospitalId: 'h1',
          requiredBy: futureIso(),
          items: [{ bloodType: 'O+', quantity: 1, bloodBankId: 'bank-a' }],
        },
        { id: 'h1', role: UserRole.ADMIN, email: 'admin@test.com' },
      ),
    ).rejects.toBeInstanceOf(UnprocessableEntityException);

    expect(inventory.releaseStockByBankAndType).toHaveBeenCalledWith(
      'bank-a',
      'O+',
      1,
    );
  });
});
