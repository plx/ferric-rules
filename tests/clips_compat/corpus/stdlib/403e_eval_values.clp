(defglobal ?*g* = 7)
(defrule run =>
  (printout t (eval "(+ 1 2)") " " (eval "?*g*") " "
    (eval word) " " (eval "(create$ a b)") crlf))
