import { Type } from 'class-transformer';
import { IsInt, IsString, Max, MaxLength, Min, MinLength } from 'class-validator';

export class CreateRequestItemDto {
  @IsString()
  @MinLength(1)
  @MaxLength(16)
  bloodType: string;

  @Type(() => Number)
  @IsInt()
  @Min(1)
  @Max(500)
  quantity: number;

  @IsString()
  @MinLength(1)
  @MaxLength(64)
  bloodBankId: string;
}
