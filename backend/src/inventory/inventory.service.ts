import { ConflictException, Injectable, NotFoundException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { InventoryStockEntity } from './entities/inventory-stock.entity';

@Injectable()
export class InventoryService {
  constructor(
    @InjectRepository(InventoryStockEntity)
    private readonly inventoryRepo: Repository<InventoryStockEntity>,
  ) {}

  async findAll(hospitalId?: string) {
    const where = hospitalId ? { bloodBankId: hospitalId } : {};
    const data = await this.inventoryRepo.find({ where });
    return {
      message: 'Inventory items retrieved successfully',
      data,
    };
  }

  async findOne(id: string) {
    const item = await this.inventoryRepo.findOne({ where: { id } });
    if (!item) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }
    return {
      message: 'Inventory item retrieved successfully',
      data: item,
    };
  }

  async create(createInventoryDto: any) {
    const existing = await this.inventoryRepo.findOne({
      where: {
        bloodBankId: createInventoryDto.bloodBankId,
        bloodType: createInventoryDto.bloodType,
      },
    });

    const entity = existing
      ? this.inventoryRepo.merge(existing, {
          availableUnits: Number(createInventoryDto.availableUnits ?? createInventoryDto.quantity ?? 0),
        })
      : this.inventoryRepo.create({
          bloodBankId: createInventoryDto.bloodBankId,
          bloodType: createInventoryDto.bloodType,
          availableUnits: Number(createInventoryDto.availableUnits ?? createInventoryDto.quantity ?? 0),
        });

    const data = await this.inventoryRepo.save(entity);
    return {
      message: 'Inventory item created successfully',
      data,
    };
  }

  async update(id: string, updateInventoryDto: any) {
    const existing = await this.inventoryRepo.findOne({ where: { id } });
    if (!existing) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }

    const updated = this.inventoryRepo.merge(existing, {
      ...updateInventoryDto,
      availableUnits:
        updateInventoryDto.availableUnits !== undefined
          ? Number(updateInventoryDto.availableUnits)
          : existing.availableUnits,
    });
    const data = await this.inventoryRepo.save(updated);
    return {
      message: 'Inventory item updated successfully',
      data,
    };
  }

  async remove(id: string) {
    const item = await this.inventoryRepo.findOne({ where: { id } });
    if (!item) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }
    await this.inventoryRepo.remove(item);
    return {
      message: 'Inventory item deleted successfully',
      data: { id },
    };
  }

  async updateStock(id: string, quantity: number) {
    const existing = await this.inventoryRepo.findOne({ where: { id } });
    if (!existing) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }
    existing.availableUnits = Number(quantity);
    const data = await this.inventoryRepo.save(existing);
    return {
      message: 'Stock updated successfully',
      data,
    };
  }

  async getLowStockItems(threshold: number = 10) {
    const data = await this.inventoryRepo
      .createQueryBuilder('inventory')
      .where('inventory.availableUnits <= :threshold', { threshold })
      .getMany();

    return {
      message: 'Low stock items retrieved successfully',
      data,
    };
  }

  async findByBankAndBloodType(
    bloodBankId: string,
    bloodType: string,
  ): Promise<InventoryStockEntity | null> {
    return this.inventoryRepo.findOne({ where: { bloodBankId, bloodType } });
  }

  async reserveStockOrThrow(
    bloodBankId: string,
    bloodType: string,
    quantity: number,
  ): Promise<void> {
    if (quantity <= 0) {
      throw new ConflictException('Requested quantity must be greater than zero.');
    }

    for (let attempt = 0; attempt < 2; attempt += 1) {
      const stock = await this.findByBankAndBloodType(bloodBankId, bloodType);

      if (!stock) {
        throw new ConflictException(
          `No inventory found for blood type ${bloodType} at blood bank ${bloodBankId}.`,
        );
      }

      if (stock.availableUnits < quantity) {
        throw new ConflictException(
          `Insufficient stock for ${bloodType} at blood bank ${bloodBankId}. Available: ${stock.availableUnits}, requested: ${quantity}.`,
        );
      }

      const updateResult = await this.inventoryRepo
        .createQueryBuilder()
        .update(InventoryStockEntity)
        .set({
          availableUnits: () => `"available_units" - ${quantity}`,
          version: () => '"version" + 1',
        })
        .where('id = :id', { id: stock.id })
        .andWhere('"version" = :version', { version: stock.version })
        .andWhere('"available_units" >= :quantity', { quantity })
        .execute();

      if (updateResult.affected === 1) {
        return;
      }

      if (attempt === 0) {
        continue;
      }

      throw new ConflictException(
        'Inventory was updated by another order request. Please retry.',
      );
    }
  }

  async getCriticalStockItems() {
    return this.getLowStockItems(5);
  }

  async getStockAggregation() {
    const data = await this.inventoryRepo
      .createQueryBuilder('inventory')
      .select('inventory.bloodType', 'bloodType')
      .addSelect('SUM(inventory.availableUnits)', 'totalUnits')
      .groupBy('inventory.bloodType')
      .getRawMany();

    return {
      message: 'Stock aggregation retrieved successfully',
      data,
    };
  }

  async getInventoryStats(hospitalId?: string) {
    const where = hospitalId ? { bloodBankId: hospitalId } : {};
    const items = await this.inventoryRepo.find({ where });
    
    const totalUnits = items.reduce((sum, item) => sum + item.availableUnits, 0);
    const lowStockCount = items.filter(item => item.availableUnits <= 10).length;

    return {
      message: 'Inventory stats retrieved successfully',
      data: {
        totalItems: items.length,
        totalUnits,
        lowStockCount,
      },
    };
  }

  async getReorderSummary() {
    const lowStock = await this.getLowStockItems(10);
    return {
      message: 'Reorder summary retrieved successfully',
      data: lowStock.data,
    };
  }

  async reserveStock(id: string, quantity: number) {
    const item = await this.inventoryRepo.findOne({ where: { id } });
    if (!item) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }
    if (item.availableUnits < quantity) {
      throw new ConflictException('Insufficient stock');
    }
    item.availableUnits -= quantity;
    const data = await this.inventoryRepo.save(item);
    return {
      message: 'Stock reserved successfully',
      data,
    };
  }

  async releaseStock(id: string, quantity: number) {
    const item = await this.inventoryRepo.findOne({ where: { id } });
    if (!item) {
      throw new NotFoundException(`Inventory item '${id}' not found`);
    }
    item.availableUnits += quantity;
    const data = await this.inventoryRepo.save(item);
    return {
      message: 'Stock released successfully',
      data,
    };
  }
}
