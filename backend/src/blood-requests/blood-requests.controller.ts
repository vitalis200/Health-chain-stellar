import { Body, Controller, Post, Req } from '@nestjs/common';
import { BloodRequestsService } from './blood-requests.service';
import { CreateBloodRequestDto } from './dto/create-blood-request.dto';
import { RequirePermissions } from '../auth/decorators/require-permissions.decorator';
import { Permission } from '../auth/enums/permission.enum';

@Controller('blood-requests')
export class BloodRequestsController {
  constructor(private readonly bloodRequestsService: BloodRequestsService) {}

  @RequirePermissions(Permission.CREATE_ORDER)
  @Post()
  create(
    @Body() dto: CreateBloodRequestDto,
    @Req() req: { user: { id: string; role: string; email: string } },
  ) {
    return this.bloodRequestsService.create(dto, req.user);
  }
}
