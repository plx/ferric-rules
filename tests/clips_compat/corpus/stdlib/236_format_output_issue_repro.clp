;; Format writes to t and returns its string, preserving surrounding printout order.
;; Level: interaction
;; Covers: format, printout, deffunction
(deffunction f (?x)
  (format t "in-deffunction x=%d%n" ?x)
  (printout t "after f" crlf))
(deffunction g () (printout t "G" crlf) 1)
(defrule r =>
  (format t "rhs n=%d%n" 42)
  (bind ?r (format t "both %s%n" abc))
  (printout t "t-return: [" ?r "]" crlf)
  (f 5)
  (printout t "a " (format t "b%n") "c" crlf)
  (printout t "x " (g) " y" crlf))
