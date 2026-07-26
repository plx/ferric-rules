; Harness for telefonica-clips/branches/64x/windows/MVS_2017/AutoFormsExample/auto_en.clp
; Detected constructs: deffacts: text-for-id
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-9957b1072db09db95f8ebfb875ffeed369695b80b3aa0699377cfd23b3fb2fd3-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-9957b1072db09db95f8ebfb875ffeed369695b80b3aa0699377cfd23b3fb2fd3|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9957b1072db09db95f8ebfb875ffeed369695b80b3aa0699377cfd23b3fb2fd3|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-9957b1072db09db95f8ebfb875ffeed369695b80b3aa0699377cfd23b3fb2fd3|COMPLETE" crlf))
