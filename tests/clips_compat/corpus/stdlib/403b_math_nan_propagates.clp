;; Domain checks reject out-of-range arguments; a NaN argument is not out of
;; range, so each function returns NaN and the rule continues. The NaN sign
;; that libm and printf produce varies by platform, so print its kind.
(deffunction nan-kind (?x)
  (if (member$ (str-cat ?x) (create$ "nan.0" "-nan.0")) then nan else ?x))
(defrule probe =>
  (bind ?n (- (exp 1000) (exp 1000)))
  (printout t "sqrt " (nan-kind (sqrt ?n)) crlf)
  (printout t "asin " (nan-kind (asin ?n)) crlf)
  (printout t "acos " (nan-kind (acos ?n)) crlf)
  (printout t "acosh " (nan-kind (acosh ?n)) crlf)
  (printout t "atanh " (nan-kind (atanh ?n)) crlf)
  (printout t "log " (nan-kind (log ?n)) crlf)
  (printout t "log10 " (nan-kind (log10 ?n)) crlf)
  (printout t "pow " (nan-kind (** ?n 0.5)) crlf)
  (printout t "after" crlf))
