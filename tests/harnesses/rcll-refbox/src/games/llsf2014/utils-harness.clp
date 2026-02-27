; Harness for rcll-refbox/src/games/llsf2014/utils.clp
; Detected constructs: defglobal: ?*DEBUG*; deffunction: debug/1, is-even-int/1, is-odd-int/1, non-zero-pose/1, in-box/3, string-gt/2
;
; Strategy: verify file loads and reset succeeds.
; The source file is loaded via (load ...) before this harness.

(defrule harness-verify
   (initial-fact)
   =>
   (printout t "HARNESS: loaded" crlf))
