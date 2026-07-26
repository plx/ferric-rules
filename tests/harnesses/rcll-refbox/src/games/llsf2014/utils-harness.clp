; Harness for rcll-refbox/src/games/llsf2014/utils.clp
; Detected constructs: defglobal: ?*DEBUG*; deffunction: debug/1, is-even-int/1, is-odd-int/1, non-zero-pose/1, in-box/3, string-gt/2
;
; Strategy: prove reset/run reaches an isolated MAIN verifier.
; The source and harness are composed and loaded together before reset.

(defrule MAIN::ferric-harness-5d75136afeab372b250c8a039b2dc0108ca9d699d18f51f4dcfa7e88b6d40306-verify
   (declare (salience 10000))
   (initial-fact)
   =>
   (printout t "FERRIC-HARNESS|2|ferric-harness-5d75136afeab372b250c8a039b2dc0108ca9d699d18f51f4dcfa7e88b6d40306|START" crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5d75136afeab372b250c8a039b2dc0108ca9d699d18f51f4dcfa7e88b6d40306|STATE|focus=" (get-focus) crlf)
   (printout t "FERRIC-HARNESS|2|ferric-harness-5d75136afeab372b250c8a039b2dc0108ca9d699d18f51f4dcfa7e88b6d40306|COMPLETE" crlf))
