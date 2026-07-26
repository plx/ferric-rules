; Harness for telefonica-clips/branches/65x/test_suite/gnrcprc.clp
; Detected constructs: deffunction: testit/0; defgeneric: slot-replace, class-slots; defmethod: t1, t2, t3, slot-replace, class-slots
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-4a0def1f16255635a3b3cecb716dc4a2865ef13157a00cd2159eef65731c70df-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-4a0def1f16255635a3b3cecb716dc4a2865ef13157a00cd2159eef65731c70df|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4a0def1f16255635a3b3cecb716dc4a2865ef13157a00cd2159eef65731c70df|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-4a0def1f16255635a3b3cecb716dc4a2865ef13157a00cd2159eef65731c70df|COMPLETE" crlf))
