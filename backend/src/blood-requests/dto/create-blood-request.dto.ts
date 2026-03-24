import { Type } from 'class-transformer';
import {
  ArrayMinSize,
  IsDateString,
  IsOptional,
  IsString,
  MaxLength,
  MinLength,
  ValidateNested,
} from 'class-validator';
import { CreateRequestItemDto } from './create-request-item.dto';

export class CreateBloodRequestDto {
  @IsString()
  @MinLength(1)
  @MaxLength(64)
  hospitalId: string;

  /** ISO 8601 datetime; must be strictly in the future at creation time. */
  @IsDateString()
  requiredBy: string;

  @ValidateNested({ each: true })
  @Type(() => CreateRequestItemDto)
  @ArrayMinSize(1)
  items: CreateRequestItemDto[];

  @IsOptional()
  @IsString()
  @MaxLength(500)
  deliveryAddress?: string;

  @IsOptional()
  @IsString()
  @MaxLength(2000)
  notes?: string;
}
