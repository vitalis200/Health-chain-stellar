import 'reflect-metadata';
import { plainToInstance } from 'class-transformer';
import { validate } from 'class-validator';
import { CreateBloodRequestDto } from './create-blood-request.dto';

describe('CreateBloodRequestDto validation', () => {
  const future = new Date(Date.now() + 60_000).toISOString();

  it('accepts valid multi-item payload', async () => {
    const dto = plainToInstance(CreateBloodRequestDto, {
      hospitalId: 'hospital-uuid-1',
      requiredBy: future,
      items: [
        { bloodType: 'O+', quantity: 1, bloodBankId: 'bank-1' },
        { bloodType: 'A-', quantity: 3, bloodBankId: 'bank-2' },
      ],
    });
    const errors = await validate(dto);
    expect(errors).toHaveLength(0);
  });

  it('rejects empty items', async () => {
    const dto = plainToInstance(CreateBloodRequestDto, {
      hospitalId: 'h1',
      requiredBy: future,
      items: [],
    });
    const errors = await validate(dto);
    expect(errors.length).toBeGreaterThan(0);
  });
});
