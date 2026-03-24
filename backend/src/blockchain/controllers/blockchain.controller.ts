import {
  Controller,
  Get,
  Post,
  Body,
  Param,
  UseGuards,
  HttpCode,
  HttpStatus,
} from '@nestjs/common';
import { SorobanService } from '../services/soroban.service';
import type { SorobanTxJob, QueueMetrics, SorobanTxResult } from '../types/soroban-tx.types';
import { AdminGuard } from '../guards/admin.guard';

@Controller('blockchain')
export class BlockchainController {
  constructor(private sorobanService: SorobanService) {}

  /**
   * Submit a transaction to the Soroban queue.
   * 
   * All contract calls must go through this endpoint.
   * Returns immediately with job ID for async status tracking.
   * 
   * @param job - Transaction job with contractMethod, args, and idempotencyKey
   * @returns Job ID for status tracking
   * @throws 400 if idempotency key already exists (duplicate submission)
   */
  @Post('submit-transaction')
  @HttpCode(HttpStatus.ACCEPTED)
  async submitTransaction(@Body() job: SorobanTxJob): Promise<{ jobId: string }> {
    const jobId = await this.sorobanService.submitTransaction(job);
    return { jobId };
  }

  /**
   * Get real-time queue metrics (admin only).
   * 
   * Protected by AdminGuard - requires admin authentication.
   * Returns current queue depth, failed jobs, and DLQ count.
   * 
   * @returns Queue metrics
   * @throws 403 if not authenticated as admin
   */
  @Get('queue/status')
  @UseGuards(AdminGuard)
  @HttpCode(HttpStatus.OK)
  async getQueueStatus(): Promise<QueueMetrics> {
    return this.sorobanService.getQueueMetrics();
  }

  /**
   * Get status of a specific job.
   * 
   * Returns current job state, error details, and retry count.
   * 
   * @param jobId - Job ID to check
   * @returns Job status or null if not found
   */
  @Get('job/:jobId')
  @HttpCode(HttpStatus.OK)
  async getJobStatus(@Param('jobId') jobId: string): Promise<SorobanTxResult | null> {
    return this.sorobanService.getJobStatus(jobId);
  }
}
