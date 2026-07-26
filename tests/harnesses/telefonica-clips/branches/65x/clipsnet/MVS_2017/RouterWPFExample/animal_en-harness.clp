; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/RouterWPFExample/animal_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-00ccc964d73c028a0cf27fe372ab989cc6eeeaedb7f6bf4af3ee63054159273c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-00ccc964d73c028a0cf27fe372ab989cc6eeeaedb7f6bf4af3ee63054159273c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-00ccc964d73c028a0cf27fe372ab989cc6eeeaedb7f6bf4af3ee63054159273c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-00ccc964d73c028a0cf27fe372ab989cc6eeeaedb7f6bf4af3ee63054159273c|COMPLETE" crlf))
