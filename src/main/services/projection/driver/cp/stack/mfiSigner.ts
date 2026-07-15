/**
 * mfiSigner — access to the Apple MFi authentication coprocessor.
 *
 * The chip lives on the i2c bus and is owned by the Python root helper, so
 * CpStack reaches it over the helper control socket (see CpHelperSock): a
 * certificate request for the accessory certificate and a sign request for a
 * signature. authSetup only depends on this interface, never on the transport,
 * so a dongle firmware could serve the same contract later.
 */

export interface MfiSigner {
  /** The accessory MFi certificate, as read from the chip. */
  certificate(): Promise<Buffer>
  /** Sign a digest with the chip's private key. */
  sign(digest: Buffer): Promise<Buffer>
}
