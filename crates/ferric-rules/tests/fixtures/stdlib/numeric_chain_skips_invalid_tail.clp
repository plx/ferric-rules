;; False comparison prefixes do not call or validate a nonnumeric tail.
(defglobal ?*trace* = 0)
(deffunction wrong () (bind ?*trace* (+ ?*trace* 1)) invalid)
(defrule probe =>
 (printout t "=:" (= 1 2 (wrong)) ":" ?*trace* crlf)
 (printout t "<>:" (<> 1 1 (wrong)) ":" ?*trace* crlf)
 (printout t "!=:" (!= 1 1 (wrong)) ":" ?*trace* crlf)
 (printout t "<:" (< 2 1 (wrong)) ":" ?*trace* crlf)
 (printout t ">:" (> 1 2 (wrong)) ":" ?*trace* crlf)
 (printout t "<=:" (<= 2 1 (wrong)) ":" ?*trace* crlf)
 (printout t ">=:" (>= 1 2 (wrong)) ":" ?*trace* crlf))
