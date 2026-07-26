; Harness for telefonica-clips/branches/63x/test_suite/gnrcprc.clp
; Detected constructs: deffunction: testit/0; defgeneric: mv-slot-replace, class-slots; defmethod: t1, t2, t3, mv-slot-replace, class-slots
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-f80acdb6057b1a975cc8e316bfe79deddaf72f6ac290d61a2d91fa2f042ab86f-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-f80acdb6057b1a975cc8e316bfe79deddaf72f6ac290d61a2d91fa2f042ab86f|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f80acdb6057b1a975cc8e316bfe79deddaf72f6ac290d61a2d91fa2f042ab86f|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-f80acdb6057b1a975cc8e316bfe79deddaf72f6ac290d61a2d91fa2f042ab86f|COMPLETE" crlf))
