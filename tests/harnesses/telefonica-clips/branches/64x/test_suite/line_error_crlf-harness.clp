; Harness for telefonica-clips/branches/64x/test_suite/line_error_crlf.clp
; Detected constructs: deffacts: points; deftemplate: point
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-decfee14dc48dbe6677da5fe5c645524195c0aabea5e3b673c971db516a2700c-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-decfee14dc48dbe6677da5fe5c645524195c0aabea5e3b673c971db516a2700c|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-decfee14dc48dbe6677da5fe5c645524195c0aabea5e3b673c971db516a2700c|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-decfee14dc48dbe6677da5fe5c645524195c0aabea5e3b673c971db516a2700c|COMPLETE" crlf))
