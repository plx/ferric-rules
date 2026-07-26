; Harness for telefonica-clips/branches/65x/test_suite/other1.clp
; Detected constructs: deffacts: wine-rules, initial-goal; deftemplate: rule
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-27db2bffc0f4ad91a719ddaad059572ddcf8b85b1a31df075be758b16aede86f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-27db2bffc0f4ad91a719ddaad059572ddcf8b85b1a31df075be758b16aede86f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-27db2bffc0f4ad91a719ddaad059572ddcf8b85b1a31df075be758b16aede86f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-27db2bffc0f4ad91a719ddaad059572ddcf8b85b1a31df075be758b16aede86f|COMPLETE" crlf))
