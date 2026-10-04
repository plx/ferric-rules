;; Printout inside deffunctions emits each argument before evaluating the next.
;; Level: interaction
;; Covers: format, printout, deffunction
(deffunction g () (printout t "G" crlf) 2)
(deffunction inner ()
  (printout t "inner " (g) " end-inner" crlf)
  1)
(deffunction outer ()
  (printout t "outer " (inner) " end-outer" crlf)
  (printout t "left " (format t "M%n") " right" crlf))
(defrule probe => (outer))
