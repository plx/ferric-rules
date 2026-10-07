;; A nil logical name in any spelling suppresses printout without evaluating its operands, and format to it only returns.
;; Level: interaction
;; Covers: format, printout, deffunction, defglobal
(defglobal ?*calls* = 0)
(deffunction mark () (bind ?*calls* (+ ?*calls* 1)) (printout t "evaluated" crlf) 1)
(deffunction quiet (?c) (printout ?c (mark)))
(defrule probe =>
  (printout nil (mark) "unseen" crlf)
  (printout "nil" (mark))
  (quiet nil)
  (quiet "nil")
  (printout t "[" (format "nil" "s=%d" 7) "]" crlf)
  (printout t "[" (format [nil] "s=%d" 8) "]" crlf)
  (printout [nil] (mark))
  (printout t "calls=" ?*calls* crlf))
