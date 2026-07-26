; Harness for telefonica-clips/branches/64x/windows/MVS_2015/AutoFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-b01ad4db9f9eb4002cb066000d25b34680b6731909db68cd215fcbe4798643b5-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-b01ad4db9f9eb4002cb066000d25b34680b6731909db68cd215fcbe4798643b5|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b01ad4db9f9eb4002cb066000d25b34680b6731909db68cd215fcbe4798643b5|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-b01ad4db9f9eb4002cb066000d25b34680b6731909db68cd215fcbe4798643b5|COMPLETE" crlf))
