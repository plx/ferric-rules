;; #341 pinned nth$ behavior: nth-alias-control

(deffacts startup (go))
(defrule exercise (go) =>
(printout t (nth 1 (create$ a b)) ":" (nth 0 (create$ a b)) crlf)
)
