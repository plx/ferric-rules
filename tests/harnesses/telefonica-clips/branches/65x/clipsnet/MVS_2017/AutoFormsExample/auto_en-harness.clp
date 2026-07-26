; Harness for telefonica-clips/branches/65x/clipsnet/MVS_2017/AutoFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-1cb607c8991991cf07cfe3140de1b891e387825787d8ff37d2b1f1dd7d73b9ae-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-1cb607c8991991cf07cfe3140de1b891e387825787d8ff37d2b1f1dd7d73b9ae|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1cb607c8991991cf07cfe3140de1b891e387825787d8ff37d2b1f1dd7d73b9ae|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-1cb607c8991991cf07cfe3140de1b891e387825787d8ff37d2b1f1dd7d73b9ae|COMPLETE" crlf))
