; Harness for telefonica-clips/branches/64x/windows/MVS_2017/RouterFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-77feb9b4582ac562138d05d7c1b3913f4241115e2b97f15b4fde39cb0d1a97d5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-77feb9b4582ac562138d05d7c1b3913f4241115e2b97f15b4fde39cb0d1a97d5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-77feb9b4582ac562138d05d7c1b3913f4241115e2b97f15b4fde39cb0d1a97d5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-77feb9b4582ac562138d05d7c1b3913f4241115e2b97f15b4fde39cb0d1a97d5|COMPLETE" crlf))
