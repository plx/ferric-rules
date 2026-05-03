(defglobal ?*threshold* = 42)

(defrule observe
    (reading ?n)
    =>
    (printout t "saw " ?n " (threshold=" ?*threshold* ")" crlf))
