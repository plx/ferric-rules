;; #341 pinned nth$ behavior: empty-and-extreme-indices

(deffacts startup (go))
(defrule exercise (go) =>
(printout t (nth$ -9223372036854775808 (create$)) crlf)
(printout t (nth$ -1 (create$)) crlf)
(printout t (nth$ 0 (create$)) crlf)
(printout t (nth$ 1 (create$)) crlf)
(printout t (nth$ 2 (create$)) crlf)
(printout t (nth$ 3 (create$)) crlf)
(printout t (nth$ 9223372036854775807 (create$)) crlf)
(printout t (nth$ -9223372036854775808 (create$ a b)) crlf)
(printout t (nth$ -1 (create$ a b)) crlf)
(printout t (nth$ 0 (create$ a b)) crlf)
(printout t (nth$ 1 (create$ a b)) crlf)
(printout t (nth$ 2 (create$ a b)) crlf)
(printout t (nth$ 3 (create$ a b)) crlf)
(printout t (nth$ 9223372036854775807 (create$ a b)) crlf)
)
