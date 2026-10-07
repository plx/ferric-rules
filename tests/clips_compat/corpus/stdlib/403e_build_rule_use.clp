(defrule run =>
  (printout t (build "(deftemplate p (slot n))") " ")
  (printout t (build "(defrule later (p (n ?n)) => (printout t later ?n crlf))") crlf)
  (assert-string "(p (n 8))"))
