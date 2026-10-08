;; Domain checks reject out-of-range arguments; a NaN argument is not out of
;; range, so each function returns NaN and the rule continues.
(defrule probe =>
  (bind ?n (- (exp 1000) (exp 1000)))
  (printout t "sqrt " (sqrt ?n) crlf)
  (printout t "asin " (asin ?n) crlf)
  (printout t "acos " (acos ?n) crlf)
  (printout t "acosh " (acosh ?n) crlf)
  (printout t "atanh " (atanh ?n) crlf)
  (printout t "log " (log ?n) crlf)
  (printout t "log10 " (log10 ?n) crlf)
  (printout t "pow " (** ?n 0.5) crlf)
  (printout t "after" crlf))
