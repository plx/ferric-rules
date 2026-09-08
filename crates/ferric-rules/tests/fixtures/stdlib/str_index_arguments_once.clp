(defglobal ?*trace* = 0)
(deffunction mark (?digit ?value) (bind ?*trace* (+ (* ?*trace* 10) ?digit)) ?value)
(defrule probe =>
 (printout t (str-index (mark 1 "") (mark 2 "abc")) ":" ?*trace* crlf)
 (bind ?*trace* 0)
 (printout t (str-index (mark 1 "b") (mark 2 "abc")) ":" ?*trace* crlf)
 (bind ?*trace* 0)
 (printout t (str-index (mark 1 "a") (mark 2 "")) ":" ?*trace* crlf))
