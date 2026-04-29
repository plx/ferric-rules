;; Template and rules for testing symbol vs string discrimination.
(deftemplate color-info
  (slot color-sym)
  (slot color-str))

(defrule detect-symbol
  (color-info (color-sym red))
  =>
  (printout t "symbol-match" crlf))

(defrule detect-string
  (color-info (color-str "red"))
  =>
  (printout t "string-match" crlf))
