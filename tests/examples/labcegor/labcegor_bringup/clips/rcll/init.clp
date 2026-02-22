(defrule init-yaml-config
  (not (yaml-loaded))
=>
  (bind ?share-dir (ament-index-get-package-share-directory "labcegor_bringup"))
  (config-load (str-cat ?share-dir "/params/game.yaml") "/")
  (assert (yaml-loaded))
  (assert (game-state (team "Carologistics")))
  (assert (game-time 0.)) 
)
